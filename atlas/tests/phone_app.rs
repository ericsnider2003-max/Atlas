//! The hub installs on a phone's home screen, and opens when it gets there.
//!
//! The phone reaches the hub over HTTPS through `tailscale serve`, which makes
//! the page installable. What makes the installed app actually *open* is the
//! part tested here, over a real socket to a real `Server`:
//!
//! - **The manifest carries the token in `start_url`.** iOS gives a home-screen
//!   app its own cookie jar, separate from Safari's, so an app that opened on
//!   plain `/hub` would open on a 401.
//! - **So the manifest is behind the token, and `?t=` opens it directly.** A
//!   manifest is fetched without the cookie, and the usual trade of `?t=` for
//!   a cookie is a 303 that a manifest fetch does not follow into anything
//!   useful.
//! - **The service worker and icons need no token.** They are fetched without
//!   the cookie too, and hold nothing private.
//! - **Every page points at all of it**, with the token in the manifest link.

use atlas::hub::{self, Page};
use atlas::server::{Action, Reply, Server, ServerConfig, COOKIE};
use std::io::{Read, Write};
use std::net::TcpStream;

const TOKEN: &str = "phonea-ppmnkq-rstuvw-xyz234";

fn a_server() -> (Server, u16) {
    let cfg = ServerConfig { enabled: true, port: 0, ..ServerConfig::default() };
    let s = Server::bind(&cfg, TOKEN).expect("bind");
    let port = s.port();
    (s, port)
}

fn ask(port: u16, raw: &str) -> Vec<u8> {
    let mut c = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    c.set_read_timeout(Some(std::time::Duration::from_secs(10))).expect("timeout");
    c.write_all(raw.as_bytes()).expect("write");
    c.flush().expect("flush");
    let mut out = Vec::new();
    let _ = c.read_to_end(&mut out);
    out
}

/// One request, served on this thread; the page handler answers `page`.
fn exchange(raw: String, page: String) -> (String, Vec<u8>) {
    let (server, port) = a_server();
    let client = std::thread::spawn(move || ask(port, &raw));
    let _ = server.serve_once(&mut |_a: Action| Reply::html(page.clone()));
    let bytes = client.join().expect("client thread");
    let split = bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("a response with a header block");
    let head = String::from_utf8_lossy(&bytes[..split]).to_string();
    (head, bytes[split + 4..].to_vec())
}

fn get(path: &str) -> (String, Vec<u8>) {
    exchange(
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nUser-Agent: Mozilla/5.0\r\n\r\n"),
        String::new(),
    )
}

fn manifest_path() -> String {
    format!("{}?t={TOKEN}", hub::MANIFEST_PATH)
}

#[test]
fn the_manifest_opens_with_the_token_in_the_address_and_is_not_a_redirect() {
    let (head, _) = get(&manifest_path());
    assert!(
        head.starts_with("HTTP/1.1 200"),
        "the manifest with the right ?t= must be answered, not traded for a cookie. \
         A manifest fetch has no cookie and does not follow a redirect into a page:\n{head}"
    );
    assert!(!head.contains("Location:"), "no redirect at all:\n{head}");
    assert!(
        head.contains("Content-Type: application/manifest+json"),
        "wrong content type:\n{head}"
    );
}

#[test]
fn the_manifest_is_json_that_opens_the_app_on_the_hub_with_the_token() {
    let (_, body) = get(&manifest_path());
    let m: serde_json::Value = serde_json::from_slice(&body).expect("the manifest is JSON");
    let start = m["start_url"].as_str().expect("start_url");
    assert!(
        start.starts_with("/hub") && start.contains(&format!("t={TOKEN}")),
        "start_url must carry the token: an installed iOS app has its own cookie \
         jar and would otherwise open on a 401. Got {start}"
    );
    assert_eq!(m["display"], "standalone");
    assert_eq!(m["scope"], "/hub");
    assert_eq!(m["id"], "/hub");
    assert_eq!(m["name"], "Atlas");
    assert_eq!(m["short_name"], "Atlas");
    assert_eq!(m["background_color"], "#f7f4ee", "the app opens on Warm Paper, the lead colourway");
    let sizes: Vec<&str> = m["icons"]
        .as_array()
        .expect("icons")
        .iter()
        .filter_map(|i| i["sizes"].as_str())
        .collect();
    assert!(sizes.contains(&"192x192") && sizes.contains(&"512x512"), "icons: {sizes:?}");
}

#[test]
fn the_manifest_with_a_wrong_token_or_none_is_refused() {
    let (head, body) = get(&format!("{}?t=not-the-token-at-all-sorry", hub::MANIFEST_PATH));
    assert!(head.starts_with("HTTP/1.1 401"), "a wrong ?t= opened the manifest:\n{head}");
    assert!(
        !String::from_utf8_lossy(&body).contains(TOKEN),
        "a refused request was handed the token"
    );

    let (head, _) = get(hub::MANIFEST_PATH);
    assert!(head.starts_with("HTTP/1.1 401"), "no token opened the manifest:\n{head}");
}

#[test]
fn the_manifest_also_opens_with_the_cookie() {
    let (head, body) = exchange(
        format!(
            "GET {} HTTP/1.1\r\nHost: 127.0.0.1\r\nCookie: {COOKIE}={TOKEN}\r\n\r\n",
            hub::MANIFEST_PATH
        ),
        String::new(),
    );
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert!(serde_json::from_slice::<serde_json::Value>(&body).is_ok());
}

#[test]
fn the_service_worker_is_served_without_a_token() {
    let (head, body) = get(hub::SERVICE_WORKER_PATH);
    assert!(head.starts_with("HTTP/1.1 200"), "the service worker needs no token:\n{head}");
    assert!(head.contains("Content-Type: text/javascript"), "{head}");
    assert!(
        head.contains("Service-Worker-Allowed: /hub"),
        "without this a worker at /hub/sw.js may not control /hub itself:\n{head}"
    );
    let js = String::from_utf8(body).expect("utf-8");
    assert!(!js.contains(TOKEN), "the public worker must hold nothing private");
    assert!(js.contains("navigate"), "it handles page loads");
    assert!(js.contains("AbortController") && js.contains("12000"), "a laptop asleep doesn't hang the page");
    assert!(js.contains("method!=='GET'"), "and leaves everything but GET alone");
    // It stores nothing on the phone: `tests/server_safety.rs` holds that a
    // phone must not cache workspace state, and a worker's cache ignores
    // the pages' `no-store`.
    assert!(!js.contains("caches.open") && !js.contains(".put("), "the worker keeps copies of pages");
    assert!(
        // Not "on your laptop" since 26 Sep: the phone app runs Atlas on the
        // phone itself, and this worker serves both.
        js.contains("I can't reach Atlas from here right now."),
        "the offline page says plainly that Atlas can't be reached"
    );
    assert!(!js.contains("http://") && !js.contains("https://"), "nothing from elsewhere");
}

/// Width and height from a PNG's IHDR chunk, after checking the signature.
fn png_size(bytes: &[u8]) -> (u32, u32) {
    assert!(bytes.len() > 24, "too short to be a PNG");
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "not a PNG signature");
    assert_eq!(&bytes[12..16], b"IHDR", "first chunk is not IHDR");
    let w = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let h = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    (w, h)
}

#[test]
fn the_icons_are_served_without_a_token_at_their_stated_sizes() {
    for (path, size) in [
        ("/hub/icon-192.png", 192),
        ("/hub/icon-512.png", 512),
        ("/hub/apple-touch-icon.png", 180),
    ] {
        let (head, body) = get(path);
        assert!(head.starts_with("HTTP/1.1 200"), "{path} needs no token:\n{head}");
        assert!(head.contains("Content-Type: image/png"), "{path}:\n{head}");
        assert!(
            head.contains(&format!("Content-Length: {}", body.len())),
            "{path}: the length header must match the bytes sent"
        );
        assert_eq!(png_size(&body), (size, size), "{path}");
    }
}

#[test]
fn only_the_public_files_skip_the_token() {
    // The exemption is a short fixed list, not a prefix.
    for path in ["/hub", "/hub/settings", "/hub/icon-999.png", "/hub/sw.js.map"] {
        let (head, _) = get(path);
        assert!(head.starts_with("HTTP/1.1 401"), "{path} was answered without a token:\n{head}");
    }
    let (head, _) = exchange(
        format!("POST {} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\n\r\n", hub::SERVICE_WORKER_PATH),
        String::new(),
    );
    assert!(head.starts_with("HTTP/1.1 401"), "only a GET is public:\n{head}");
}

#[test]
fn every_page_points_at_the_app_with_the_token() {
    let page = hub::shell_at(Some(Page::Settings), "Settings", "<p>body</p>");
    let (head, body) = exchange(
        format!(
            "GET /hub/settings HTTP/1.1\r\nHost: 127.0.0.1\r\nCookie: {COOKIE}={TOKEN}\r\n\r\n"
        ),
        page,
    );
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    let html = String::from_utf8(body).expect("utf-8");
    let head_part = &html[..html.find("</head>").expect("a head")];
    for needle in [
        format!("<link rel=manifest href=\"{}?t={TOKEN}\">", hub::MANIFEST_PATH),
        "name=apple-mobile-web-app-capable content=yes".to_string(),
        "name=mobile-web-app-capable content=yes".to_string(),
        "name=apple-mobile-web-app-title content=Atlas".to_string(),
        "name=theme-color".to_string(),
        "rel=apple-touch-icon href=\"/hub/apple-touch-icon.png\"".to_string(),
        "navigator.serviceWorker.register('/hub/sw.js',{scope:'/hub'})".to_string(),
    ] {
        assert!(head_part.contains(&needle), "the page head is missing {needle}:\n{head_part}");
    }
    assert_eq!(
        html.matches("rel=manifest").count(),
        1,
        "the tags go in once, at one place"
    );
}

#[test]
fn a_fragment_with_no_head_is_left_alone() {
    let (_, body) = exchange(
        format!("GET /hub HTTP/1.1\r\nHost: 127.0.0.1\r\nCookie: {COOKIE}={TOKEN}\r\n\r\n"),
        "<p>just a line</p>".into(),
    );
    assert_eq!(String::from_utf8(body).unwrap(), "<p>just a line</p>");
}
