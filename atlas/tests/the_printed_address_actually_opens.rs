//! The address Atlas prints has to open in a browser.
//!
//! ## What was wrong
//!
//! `server::hub_url` prints `http://127.0.0.1:PORT/hub?t=TOKEN`. That address
//! was **dead**, for two independent reasons, and it had been dead since the
//! token was introduced:
//!
//! 1. `parse_request` read the token from `X-Atlas-Token` and
//!    `Authorization: Bearer` and nothing else. A browser sends neither, so
//!    the token was `None` and every request answered **401**.
//! 2. The raw target became the path, so `hub::route` was asked to match
//!    `"/hub?t=…"` against `"/hub"` and did not. Even with a token it was a
//!    **404**.
//!
//! What that cost: the daemon's own startup line ("your dashboard: …"),
//! `atlas hub`, and both addresses `atlas settings` prints — which is
//! ATLAS.bat's menu item 3, the one labelled *"works even when Atlas won't"*.
//! The break-glass recovery path was the loudest thing broken. Every form
//! button on every hub page would have answered 401 too, since no form
//! carried a header either.
//!
//! ## Why the existing test did not catch it
//!
//! `tests/the_hub_is_reachable.rs` splits `?t=` out of the URL **string** and
//! hands the two halves to `token_matches` directly. It never constructs a
//! request, so it asserts nothing about what a browser gets — while its own
//! comment says it exists *"so a change to how the token is carried fails
//! here rather than silently printing something refused."* That is this
//! file's reason for existing: **everything here goes over a real socket to a
//! real `Server`**, because the defect lived entirely in the space between
//! "the pieces are correct" and "the request works".
//!
//! ## The cookie
//!
//! A token in a URL goes into browser history and into the `Referer` of every
//! link the page holds. So the first request trades it for a
//! `SameSite=Strict` cookie and redirects to the clean path.
//!
//! `SameSite=Strict` is not decoration. The header-only scheme was
//! CSRF-proof by accident — no browser sends a custom header unasked — and a
//! cookie the browser sends automatically would have given that away.
//! `Strict` means the browser does not send it on a cross-site request at
//! all, so the property is now held on purpose. `the_cookie_is_not_sent_across_sites`
//! is the check that it stays that way.

use atlas::server::{Action, Reply, Server, ServerConfig, COOKIE};
use std::io::{Read, Write};
use std::net::TcpStream;

const TOKEN: &str = "abcdef-ghjkmn-pqrstu-vwxyz2";

fn a_server() -> (Server, u16) {
    // Port 0: the OS picks a free one, so these tests never collide with a
    // running Atlas or with each other.
    let cfg = ServerConfig { enabled: true, port: 0, ..ServerConfig::default() };
    let s = Server::bind(&cfg, TOKEN).expect("bind");
    let port = s.port();
    (s, port)
}

/// Send one raw request and return the whole response.
fn ask(port: u16, raw: &str) -> String {
    let mut c = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    c.set_read_timeout(Some(std::time::Duration::from_secs(10))).expect("timeout");
    c.write_all(raw.as_bytes()).expect("write");
    c.flush().expect("flush");
    let mut out = String::new();
    let _ = c.read_to_string(&mut out);
    out
}

/// Serve exactly one request on this thread while a client talks to it.
fn one_exchange(server: Server, port: u16, raw: String) -> (String, Option<Action>) {
    let client = std::thread::spawn(move || ask(port, &raw));
    let mut seen = None;
    let action = server.serve_once(&mut |a: Action| {
        seen = Some(a);
        Reply::html("<h1>the hub</h1>")
    });
    let response = client.join().expect("client thread");
    (response, action.ok().flatten().or(seen))
}

/// What a browser sends, given an address — and nothing a browser would not.
fn as_a_browser_would(url: &str) -> String {
    let target = url.split_once("127.0.0.1").and_then(|(_, r)| r.split_once('/')).map(|(_, p)| format!("/{p}")).expect("a url with a path");
    format!(
        "GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\
         User-Agent: Mozilla/5.0\r\n\
         Accept: text/html\r\n\r\n"
    )
}

#[test]
fn the_address_atlas_prints_is_accepted() {
    let (server, port) = a_server();
    let url = atlas::server::hub_url(port, TOKEN, "/hub");
    let (response, _) = one_exchange(server, port, as_a_browser_would(&url));

    assert!(
        !response.starts_with("HTTP/1.1 401"),
        "the address Atlas prints was refused. Every place it is printed -- the \
         daemon's startup line, `atlas hub`, and ATLAS.bat's \"works even when \
         Atlas won't\" -- is then a dead end.\n{response}"
    );
    assert!(
        !response.starts_with("HTTP/1.1 404"),
        "the token was accepted and the path was not found, which means the query \
         string is still part of the path:\n{response}"
    );
    assert!(
        response.starts_with("HTTP/1.1 200") && response.contains("Set-Cookie: "),
        "expected the token to be traded for a cookie:\n{response}"
    );
}

#[test]
fn the_first_visit_puts_the_token_in_a_cookie_and_cleans_the_address() {
    let (server, port) = a_server();
    let url = atlas::server::hub_url(port, TOKEN, "/hub");
    let (response, _) = one_exchange(server, port, as_a_browser_would(&url));

    assert!(
        response.contains(&format!("Set-Cookie: {COOKIE}={TOKEN}")),
        "no cookie was set, so the token stays in the address bar and in the \
         Referer of every link on the page:\n{response}"
    );
    // A page that moves itself on rather than a 303: a phone opening the
    // address from its camera arrives cross-site, and browsers keep a Strict
    // cookie from a redirect such a visit started (27 Sep 2026, Eric's
    // iPhone got "denied"). A hop the page makes itself starts on this site.
    assert!(
        response.contains("location.replace('/hub')") && response.contains("url=/hub'"),
        "the page doesn't go on to the clean path:\n{response}"
    );
    assert!(
        !response.contains("/hub?t="),
        "the hop carries the token straight back into the URL:\n{response}"
    );
}

#[test]
fn the_cookie_is_not_sent_across_sites() {
    // The whole reason a cookie is safe here. Asserted on the header Atlas
    // emits, because the enforcement is the browser's and the only thing
    // this side controls is asking for it.
    let (server, port) = a_server();
    let url = atlas::server::hub_url(port, TOKEN, "/hub");
    let (response, _) = one_exchange(server, port, as_a_browser_would(&url));

    assert!(
        response.contains("SameSite=Strict"),
        "the cookie is not SameSite=Strict, so any page on any other origin can \
         submit a form to the hub with the browser attaching the credential. The \
         header-only scheme had this property by accident; a cookie without \
         Strict gives it away:\n{response}"
    );
    assert!(
        response.contains("HttpOnly"),
        "the cookie is readable by script on a page that renders note titles, \
         device names and app paths:\n{response}"
    );
}

#[test]
fn the_cookie_alone_opens_the_hub_on_the_next_request() {
    // The second half of the handoff: with the cookie and no token in the
    // address, the page has to render.
    let (server, port) = a_server();
    let raw = format!(
        "GET /hub HTTP/1.1\r\nHost: 127.0.0.1\r\n\
         User-Agent: Mozilla/5.0\r\nCookie: {COOKIE}={TOKEN}\r\n\r\n"
    );
    let (response, action) = one_exchange(server, port, raw);

    assert!(
        response.starts_with("HTTP/1.1 200"),
        "the cookie this server set does not open the page it set it for:\n{response}"
    );
    assert!(action.is_some(), "the request authenticated and routed to nothing");
}

#[test]
fn a_form_post_works_with_the_cookie() {
    // No hub form carries a token, so with a header-only scheme every button
    // on every page answered 401. This is the check that the cookie makes
    // them work.
    let (server, port) = a_server();
    let body = "key=hub.theme&value=dark";
    let raw = format!(
        "POST /hub/set HTTP/1.1\r\nHost: 127.0.0.1\r\n\
         Cookie: {COOKIE}={TOKEN}\r\n\
         Content-Type: application/x-www-form-urlencoded\r\n\
         Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let (response, action) = one_exchange(server, port, raw);

    assert!(
        !response.starts_with("HTTP/1.1 401"),
        "a form submitted from a hub page was refused:\n{response}"
    );
    assert!(
        matches!(action, Some(Action::HubSet { .. })),
        "the form did not reach the setting handler: {action:?}"
    );
}

#[test]
fn a_wrong_token_in_the_address_is_still_refused() {
    let (server, port) = a_server();
    let url = atlas::server::hub_url(port, "not-the-token", "/hub");
    let (response, action) = one_exchange(server, port, as_a_browser_would(&url));
    assert!(
        response.starts_with("HTTP/1.1 401"),
        "a wrong token in the query string was accepted:\n{response}"
    );
    assert!(action.is_none(), "a refused request reached a handler: {action:?}");
}

#[test]
fn no_token_at_all_is_refused() {
    let (server, port) = a_server();
    let (response, action) = one_exchange(
        server,
        port,
        "GET /hub HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n".to_string(),
    );
    assert!(response.starts_with("HTTP/1.1 401"), "{response}");
    assert!(action.is_none());
}

#[test]
fn a_header_still_works_for_the_phone_client() {
    // The phone and the peer door send a header deliberately. Adding the
    // query and the cookie must not have taken that away.
    let (server, port) = a_server();
    let raw = format!(
        "GET /status HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Atlas-Token: {TOKEN}\r\n\r\n"
    );
    let (response, action) = one_exchange(server, port, raw);
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(matches!(action, Some(Action::Status)));
}

#[test]
fn a_junk_authorization_header_does_not_cancel_a_valid_token() {
    // The old loop assigned on every matching header rather than keeping the
    // first, and the `authorization` arm assigned `strip_prefix("Bearer ")`
    // -- i.e. `None` for anything else. So a valid `X-Atlas-Token` followed
    // by `Authorization: Basic …` was a valid request refused by header
    // order.
    let (server, port) = a_server();
    let raw = format!(
        "GET /status HTTP/1.1\r\nHost: 127.0.0.1\r\n\
         X-Atlas-Token: {TOKEN}\r\n\
         Authorization: Basic Zm9vOmJhcg==\r\n\r\n"
    );
    let (response, _) = one_exchange(server, port, raw);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "header order decided the answer:\n{response}"
    );
}

#[test]
fn a_typo_in_a_pasted_address_is_a_404_not_a_redirect_loop() {
    // The cookie handoff happens only for a path that exists. Otherwise a
    // mistyped address would redirect to itself, set a cookie, and 404 on
    // the second request -- two round trips to say what one could.
    let (server, port) = a_server();
    let url = atlas::server::hub_url(port, TOKEN, "/hubb");
    let (response, _) = one_exchange(server, port, as_a_browser_would(&url));
    assert!(
        response.starts_with("HTTP/1.1 404"),
        "a nonexistent path with a valid token did not answer 404:\n{response}"
    );
}

#[test]
fn an_endless_header_line_is_cut_off_rather_than_buffered() {
    // The head limit used to be checked AFTER `read_line` returned, which is
    // after the whole line is in memory. One request of
    // `GET / HTTP/1.1\r\nX: ` followed by an endless stream containing no
    // newline grew the process without bound and answered 413 only once the
    // client stopped -- measured at 1.5GB resident from a single
    // unauthenticated connection. Repeat it and the process holding the
    // vault and the mail credentials is OOM-killed.
    let (server, port) = a_server();
    let mut raw = String::from("GET /status HTTP/1.1\r\nX: ");
    raw.push_str(&"A".repeat(64 * 1024)); // no CRLF anywhere after it
    let (response, action) = one_exchange(server, port, raw);

    assert!(
        response.is_empty() || response.starts_with("HTTP/1.1 413") || response.starts_with("HTTP/1.1 401"),
        "an oversized header block was neither refused nor cut off:\n{}",
        &response[..response.len().min(120)]
    );
    assert!(action.is_none(), "an oversized request reached a handler");
}

#[test]
fn a_client_that_says_nothing_does_not_hold_the_tick_for_long() {
    // `poll_once` runs inline in `Daemon::run`. A connection that sends
    // nothing used to hold it for the full read timeout -- and because that
    // timeout was per-read rather than per-request, a client dripping one
    // byte before each expiry held it indefinitely. For that time Atlas does
    // not listen, does not answer, does not serve the hub and does not
    // collect a message from another Atlas.
    let (server, port) = a_server();
    let held = std::thread::spawn(move || {
        let c = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        // Connect and say nothing at all, then hold the socket open.
        std::thread::sleep(std::time::Duration::from_secs(12));
        drop(c);
    });

    let started = std::time::Instant::now();
    let _ = server.serve_once(&mut |_| Reply::ok("{}"));
    let took = started.elapsed();

    assert!(
        took < std::time::Duration::from_secs(9),
        "one silent connection held the listener for {took:?}. That is time in \
         which the whole assistant is doing nothing."
    );
    let _ = held.join();
}

#[test]
fn an_unauthenticated_upload_does_not_get_a_28mb_buffer() {
    // The body used to be read before the token was checked, with
    // `vec![0u8; len]` sized from the request's own Content-Length and
    // capped at `max_upload` -- 28MB by default. So an unauthenticated
    // `POST /hand/file` cost 28MB and a five-second hold, for free, from any
    // local process.
    //
    // The check is the timing: the request declares a body it never sends,
    // so if the server reads the body first it waits for bytes that are not
    // coming. Refusing on the token alone answers at once.
    let (server, port) = a_server();
    let raw = "POST /hand/file HTTP/1.1\r\nHost: 127.0.0.1\r\n\
               Content-Length: 29360128\r\n\r\n"
        .to_string();

    let client = std::thread::spawn(move || {
        let mut c = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        c.set_read_timeout(Some(std::time::Duration::from_secs(10))).expect("t");
        c.write_all(raw.as_bytes()).expect("write");
        c.flush().expect("flush");
        let mut out = String::new();
        let _ = c.read_to_string(&mut out);
        out
    });

    let started = std::time::Instant::now();
    let action = server.serve_once(&mut |_| Reply::ok("{}"));
    let took = started.elapsed();
    let response = client.join().expect("client");

    assert!(
        response.starts_with("HTTP/1.1 401"),
        "an unauthenticated upload was not refused outright:\n{response}"
    );
    assert!(
        took < std::time::Duration::from_secs(1),
        "the unauthenticated upload was refused after {took:?} -- the body is \
         still being read before the token is checked"
    );
    assert!(action.ok().flatten().is_none());
}

#[test]
fn every_address_hub_url_can_print_routes() {
    // `hub_url` is called with several paths across `main.rs`. Each one has
    // to be a path `route` recognises, or the printed address 404s with a
    // valid token -- which is the second half of the original defect and
    // the half a token fix alone would have left in place.
    for path in ["/hub", "/hub/settings", "/hub/access", "/hub/status"] {
        let url = atlas::server::hub_url(4173, TOKEN, path);
        let head = as_a_browser_would(&url);
        let req = atlas::server::parse_request(&head, "").expect("parses");
        assert_eq!(req.path, path, "the query is still stuck to the path");
        assert_eq!(
            req.token.as_deref(),
            Some(TOKEN),
            "the token in {url} is not read out of the address"
        );
        assert!(
            atlas::hub::route(&req.path).is_some() || atlas::server::route(&req).is_some(),
            "{path} is printed by `hub_url` somewhere and is not a route"
        );
    }
}

/// A browser that isn't signed in is told what to do, in words -- not
/// `{"error":"denied"}` with no way on (27 Sep 2026, Eric's iPhone).
#[test]
fn a_browser_without_the_token_is_told_what_to_do() {
    let (server, port) = a_server();
    let raw = "GET /hub HTTP/1.1\r\nHost: 127.0.0.1\r\nUser-Agent: Mozilla/5.0\r\nAccept: text/html\r\n\r\n".to_string();
    let (response, action) = one_exchange(server, port, raw);
    assert!(action.is_none(), "an unsigned browser reached the hub");
    assert!(response.starts_with("HTTP/1.1 401"), "{response}");
    assert!(response.contains("text/html") && response.contains("isn't signed in") && response.contains("Your phone"), "{response}");
    // The app and peers still get the bare answer, which says nothing more.
    let (server, port) = a_server();
    let raw = "GET /hub/live.json HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n".to_string();
    let (response, _) = one_exchange(server, port, raw);
    assert!(response.contains("{\"error\":\"denied\"}"), "{response}");
}

/// A page or a form that nothing answers says so, with the way back, rather
/// than a bare "no such endpoint" (27 Sep 2026: the Improvements page's
/// buttons posted where nothing answered).
#[test]
fn nothing_answering_a_page_or_a_form_still_leaves_a_way_back() {
    for raw in [
        format!("GET /hub/no-such-page HTTP/1.1\r\nHost: 127.0.0.1\r\nCookie: {COOKIE}={TOKEN}\r\n\r\n"),
        format!("POST /hub/no-such-form HTTP/1.1\r\nHost: 127.0.0.1\r\nCookie: {COOKIE}={TOKEN}\r\nContent-Length: 6\r\n\r\nwhat=x"),
    ] {
        let (server, port) = a_server();
        let (response, _) = one_exchange(server, port, raw);
        assert!(response.starts_with("HTTP/1.1 404"), "{response}");
        assert!(response.contains("Back to Atlas") && response.contains("href='/hub'"), "{response}");
    }
}

/// Every form and button in the hub posts somewhere the server answers. The
/// Improvements page's "Have a go" posted to a path nothing routed, and the
/// only sign was a bare error on Eric's screen.
#[test]
fn every_form_in_the_hub_posts_somewhere_that_answers() {
    let server_src = crate::common::source_of("server");
    let mut missing = Vec::new();
    for f in ["src/hub.rs", "src/hubpages.rs", "src/hublive.rs", "src/appearance.rs", "src/palette.rs"] {
        let src = crate::common::read_source_path(f).unwrap_or_default();
        for part in src.split("action=").skip(1) {
            let p: String = part.trim_start_matches(['\'', '"', '\\']).chars().take_while(|c| c.is_ascii_alphanumeric() || "/-_.".contains(*c)).collect();
            if !p.starts_with('/') {
                continue;
            }
            if !server_src.contains(&format!("\"{p}\"")) {
                missing.push(format!("{f}: {p}"));
            }
        }
    }
    missing.sort();
    missing.dedup();
    assert!(missing.is_empty(), "forms post where the server answers nothing:\n{}", missing.join("\n"));
}
