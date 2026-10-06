//! Minimal HTTP/1.1 client for localhost.
//!
//! Needed for exactly one thing: asking Chrome's debugger endpoint which tabs
//! exist, so we can find a websocket URL to attach to. Plaintext, loopback,
//! no redirects, no TLS. A general HTTP client would be a much larger
//! dependency for a job this small.

use crate::error::{AtlasError, Result};
use std::io::Write;
use std::net::TcpStream;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    pub status: u16,
    pub body: String,
    /// The `Location` header, for a redirect the caller may choose to follow
    /// (`feeds::fetch` does, three hops at most, https only).
    pub location: Option<String>,
}

impl Response {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

pub fn get(host: &str, path: &str, timeout: Duration) -> Result<Response> {
    request("GET", host, path, None, timeout)
}

pub fn post_json(host: &str, path: &str, body: &str, timeout: Duration) -> Result<Response> {
    request("POST", host, path, Some(body), timeout)
}

/// The same, with a bearer token in the header where it belongs.
///
/// Not an optional argument on `post_json`, and not a field in the body. A
/// credential in a JSON body is a credential that ends up in whatever logs
/// the body — and, more immediately, `server::parse_request` reads the token
/// from `Authorization` or `X-Atlas-Token` and nowhere else, so a body field
/// named "token" simply is not a token. The first version of
/// the first handoff sender did exactly that and every send came back 401; only
/// an end-to-end test against the real listener caught it, because both
/// halves were individually correct.
pub fn post_json_with_token(
    host: &str,
    path: &str,
    body: &str,
    token: &str,
    timeout: Duration,
) -> Result<Response> {
    with_header("POST", host, path, Some(body), Some(("Authorization", &format!("Bearer {token}"))), timeout)
}

pub fn request(
    method: &str,
    host: &str,
    path: &str,
    body: Option<&str>,
    timeout: Duration,
) -> Result<Response> {
    with_header(method, host, path, body, None, timeout)
}

/// Read something from a machine that wants the token first.
///
/// `elsewhere.rs` asks another Atlas for its brief over the hub API, which
/// authenticates every request. Added with its caller, not ahead of one --
/// the note below is about entry points that exist to be counted.
pub fn get_with_token(
    host: &str,
    path: &str,
    token: &str,
    timeout: Duration,
) -> Result<Response> {
    with_header("GET", host, path, None, Some(("Authorization", &format!("Bearer {token}"))), timeout)
}

/// Private on purpose. `request` and `post_json_with_token` are the two
/// shapes anything outside this module needs, and a third public entry point
/// whose only caller is in this file is a capability that exists to be
/// counted rather than used.
fn with_header(
    method: &str,
    host: &str,
    path: &str,
    body: Option<&str>,
    header: Option<(&str, &str)>,
    timeout: Duration,
) -> Result<Response> {
    let addr = host
        .parse()
        .or_else(|_| {
            use std::net::ToSocketAddrs;
            host.to_socket_addrs().ok().and_then(|mut a| a.next()).ok_or(())
        })
        .map_err(|_| AtlasError::Platform(format!("cannot resolve {host}")))?;

    let mut s = TcpStream::connect_timeout(&addr, timeout)
        .map_err(|e| AtlasError::Platform(format!("connect {host}: {e}")))?;
    s.set_read_timeout(Some(timeout))?;
    s.set_write_timeout(Some(timeout))?;
    let mut req = build_request(method, host, path, body);
    if let Some((k, v)) = header {
        // Inserted after the request line rather than appended, since the
        // body (if any) is already on the end of what `build_request`
        // produced and a header after it is not a header.
        let at = req.find("\r\n").map(|i| i + 2).unwrap_or(0);
        req.insert_str(at, &format!("{k}: {v}\r\n"));
    }
    s.write_all(req.as_bytes())?;

    let raw = read_bounded(&mut s, MAX_RESPONSE)?;
    // From the BYTES, not from a string.
    //
    // This was `parse_response(&String::from_utf8_lossy(&raw))`, and
    // `dechunk`'s own first comment is *"Bytes, not characters. A chunk size
    // on the wire counts bytes"* — which was true of `dechunk` and not of
    // what it was handed. Every byte the lossy conversion could not decode
    // became U+FFFD, three bytes where one arrived, so from the first such
    // byte onward every chunk-size header pointed at the wrong offset and the
    // decoder walked off into the middle of the data.
    //
    // The fix was applied one level too shallow. The conversion belongs after
    // reassembly, which is exactly what `dechunk` says two lines further
    // down, and now that is where it happens.
    parse_response(&raw)
}

/// A POST over a connection someone else already opened -- a stream Tor
/// carried to a friend's Atlas (`onion::connect`). `host` is only what goes
/// in the Host line.
pub fn post_over(stream: &mut TcpStream, host: &str, path: &str, body: &str, timeout: Duration) -> Result<Response> {
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.write_all(build_request("POST", host, path, Some(body)).as_bytes())?;
    parse_response(&read_bounded(stream, MAX_RESPONSE)?)
}

/// How a request on a kept-open connection went.
#[derive(Debug)]
pub enum KeptReply {
    /// It answered; `true` if the far side will take the next request on
    /// the same connection.
    Answered(Response, bool),
    /// Nothing at all came back: the far side had already closed this
    /// connection while it sat idle. Nothing was read, so it's safe to send
    /// again on a new one.
    Closed,
}

/// A POST that asks to keep the connection open for the next one (Atlas's
/// Tor connections to friends, gap AN): the reply is read by its length,
/// not by the connection closing.
pub fn post_kept(stream: &mut TcpStream, host: &str, path: &str, body: &str, timeout: Duration) -> Result<KeptReply> {
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let req = build_request("POST", host, path, Some(body)).replacen("Connection: close\r\n", "Connection: keep-alive\r\n", 1);
    if stream.write_all(req.as_bytes()).is_err() {
        return Ok(KeptReply::Closed);
    }
    let raw = match read_bounded(stream, MAX_RESPONSE) {
        Ok(raw) if raw.is_empty() => return Ok(KeptReply::Closed),
        Ok(raw) => raw,
        Err(AtlasError::Io(e)) if matches!(e.kind(), std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe) => {
            return Ok(KeptReply::Closed)
        }
        Err(e) => return Err(e),
    };
    let head_end = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap_or(raw.len());
    let again = String::from_utf8_lossy(&raw[..head_end]).to_ascii_lowercase().contains("\r\nconnection: keep-alive");
    Ok(KeptReply::Answered(parse_response(&raw)?, again && whole_reply(&raw)))
}

/// A GET over TLS.
///
/// Everything else in this module is plain TCP, because everything it talked
/// to was on this machine or on your own network -- Chrome's debug port,
/// another Atlas over a pairing, a phone. A messaging service is neither, and
/// its interface is HTTPS only.
///
/// `native_tls` rather than a Rust TLS stack, for the reason `Cargo.toml`
/// already gives about IMAP: it uses the system's library, which is lighter
/// than vendoring a second crypto backend for one socket's worth of TLS.
///
/// Certificate verification is `native_tls`'s default and is not turned down.
/// There is no setting for that and there should not be: a switch for
/// accepting bad certificates is a switch that ends up on.
pub fn https_get(host: &str, path: &str, timeout: Duration) -> Result<Response> {
    https_get_with(host, path, &[], timeout)
}

/// `https_get` with extra request headers -- a `User-Agent` that says who is
/// asking, which Reddit answers and a bare request gets refused with 429
/// (the opportunity hunter, 29 Sep 2026). Same verification rules.
pub fn https_get_with(host: &str, path: &str, headers: &[(&str, &str)], timeout: Duration) -> Result<Response> {
    use std::net::ToSocketAddrs;
    let addr = format!("{host}:443")
        .to_socket_addrs()
        .ok()
        .and_then(|mut a| a.next())
        .ok_or_else(|| AtlasError::Platform(format!("cannot resolve {host}")))?;

    let tcp = TcpStream::connect_timeout(&addr, timeout)
        .map_err(|e| AtlasError::Platform(format!("connect {host}: {e}")))?;
    tcp.set_read_timeout(Some(timeout))?;
    tcp.set_write_timeout(Some(timeout))?;

    let connector = native_tls::TlsConnector::new()
        .map_err(|e| AtlasError::Platform(format!("couldn't set up TLS: {e}")))?;
    let mut s = connector
        .connect(host, tcp)
        .map_err(|e| AtlasError::Platform(format!("TLS to {host}: {e}")))?;

    let mut req = build_request("GET", host, path, None);
    let at = req.find("\r\n").map(|i| i + 2).unwrap_or(0);
    for (k, v) in headers {
        // A header is one line: anything that could start another is refused
        // rather than sent.
        if k.contains(['\r', '\n', ':']) || v.contains(['\r', '\n']) {
            return Err(AtlasError::Platform(format!("a request header for {host} had a line break in it")));
        }
        req.insert_str(at, &format!("{k}: {v}\r\n"));
    }
    s.write_all(req.as_bytes())?;
    let raw = read_bounded(&mut s, MAX_RESPONSE)?;
    parse_response(&raw)
}

/// A POST over TLS, with an optional bearer token. For a push server that
/// only speaks https (a public ntfy, or your own behind a certificate). Same
/// verification rules as `https_get`: the system's, never turned down.
pub fn https_post_json(host: &str, path: &str, body: &str, token: Option<&str>, timeout: Duration) -> Result<Response> {
    use std::net::ToSocketAddrs;
    let (name, port) = match host.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) => (h, p.parse::<u16>().unwrap_or(443)),
        _ => (host, 443),
    };
    let addr = (name, port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut a| a.next())
        .ok_or_else(|| AtlasError::Platform(format!("cannot resolve {name}")))?;
    let tcp = TcpStream::connect_timeout(&addr, timeout)
        .map_err(|e| AtlasError::Platform(format!("connect {name}: {e}")))?;
    tcp.set_read_timeout(Some(timeout))?;
    tcp.set_write_timeout(Some(timeout))?;
    let connector = native_tls::TlsConnector::new()
        .map_err(|e| AtlasError::Platform(format!("couldn't set up TLS: {e}")))?;
    let mut s = connector
        .connect(name, tcp)
        .map_err(|e| AtlasError::Platform(format!("TLS to {name}: {e}")))?;
    let mut req = build_request("POST", name, path, Some(body));
    if let Some(t) = token {
        let at = req.find("\r\n").map(|i| i + 2).unwrap_or(0);
        req.insert_str(at, &format!("Authorization: Bearer {t}\r\n"));
    }
    s.write_all(req.as_bytes())?;
    let raw = read_bounded(&mut s, MAX_RESPONSE)?;
    parse_response(&raw)
}

/// A request over TLS that says who is asking and hands back the headers.
///
/// For the public sites `social` reads (29 Sep 2026): a feed server asks for
/// a descriptive `User-Agent`, answers `If-Modified-Since` with a cheap 304,
/// and says `Retry-After` when it wants you to slow down -- none of which
/// `https_get` can send or see. `body` is `(content type, text)`: Google's
/// token endpoint takes a form, not JSON. Same certificate rules as
/// `https_get`: the system's, never turned down.
pub fn https_call(
    method: &str,
    host: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<(&str, &str)>,
    timeout: Duration,
) -> Result<(Response, Vec<(String, String)>)> {
    https_call_bytes(method, host, path, headers, body.map(|(kind, text)| (kind, text.as_bytes())), timeout)
}

/// `https_call` with a body of any bytes: a picture uploaded as itself
/// (Bluesky's `uploadBlob`), not as text.
pub fn https_call_bytes(
    method: &str,
    host: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<(&str, &[u8])>,
    timeout: Duration,
) -> Result<(Response, Vec<(String, String)>)> {
    use std::net::ToSocketAddrs;
    let addr = format!("{host}:443")
        .to_socket_addrs()
        .ok()
        .and_then(|mut a| a.next())
        .ok_or_else(|| AtlasError::Platform(format!("cannot resolve {host}")))?;
    let tcp = TcpStream::connect_timeout(&addr, timeout)
        .map_err(|e| AtlasError::Platform(format!("connect {host}: {e}")))?;
    tcp.set_read_timeout(Some(timeout))?;
    tcp.set_write_timeout(Some(timeout))?;
    let connector = native_tls::TlsConnector::new()
        .map_err(|e| AtlasError::Platform(format!("couldn't set up TLS: {e}")))?;
    let mut s = connector
        .connect(host, tcp)
        .map_err(|e| AtlasError::Platform(format!("TLS to {host}: {e}")))?;
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nAccept: */*\r\n");
    for (k, v) in headers {
        // A header value with a line break in it is a second header someone
        // else wrote; refused rather than sent.
        if k.contains(['\r', '\n']) || v.contains(['\r', '\n']) {
            return Err(AtlasError::Platform("a header with a line break in it".into()));
        }
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    if let Some((kind, _)) = body {
        if kind.contains(['\r', '\n']) {
            return Err(AtlasError::Platform("a header with a line break in it".into()));
        }
    }
    let mut wire = req.into_bytes();
    match body {
        Some((kind, bytes)) => {
            wire.extend_from_slice(format!("Content-Type: {kind}\r\nContent-Length: {}\r\n\r\n", bytes.len()).as_bytes());
            wire.extend_from_slice(bytes);
        }
        None => wire.extend_from_slice(b"\r\n"),
    }
    s.write_all(&wire)?;
    let raw = read_bounded(&mut s, MAX_RESPONSE)?;
    let head_end = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap_or(raw.len());
    let head = String::from_utf8_lossy(&raw[..head_end]).into_owned();
    let got: Vec<(String, String)> = head
        .lines()
        .skip(1)
        .filter_map(|l| l.split_once(':').map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())))
        .collect();
    Ok((parse_response(&raw)?, got))
}

/// The most Atlas will hold from one response.
///
/// `read_to_end` had no limit. The read timeout does not bound the total: a
/// server that sends a little every second keeps resetting it, and the buffer
/// grows until the process dies. Atlas talks HTTP to Chrome's debug port, to
/// another Atlas over a pairing, and to a phone — the first of which returns
/// screenshot payloads, so the ceiling has to be generous rather than tight.
///
/// 64 MiB, the same figure `imap::MAX_BUFFER` and `smtp::MAX_BUFFER` settled
/// on, for the same reason: large enough that nothing legitimate hits it,
/// small enough that hitting it is survivable.
pub const MAX_RESPONSE: usize = 64 * 1024 * 1024;

/// Read until the peer closes, or until `max` bytes have arrived.
///
/// Refuses at the limit rather than truncating. A response cut off at exactly
/// 64 MiB and handed back as if it were whole is a wrong answer that looks
/// like a right one — the same reasoning as `dechunk` dropping an incomplete
/// final chunk rather than including it.
fn read_bounded<R: std::io::Read>(s: &mut R, max: usize) -> Result<Vec<u8>> {
    let mut out: Vec<u8> = Vec::new();
    let mut buf = [0u8; 16 * 1024];
    loop {
        let n = s.read(&mut buf)?;
        if n == 0 {
            return Ok(out);
        }
        if out.len() + n > max {
            return Err(AtlasError::Platform(format!(
                "that reply is bigger than the {} MB I'll hold in memory, so I stopped \
                 reading rather than filling the machine with it",
                max / (1024 * 1024)
            )));
        }
        out.extend_from_slice(&buf[..n]);
        // A server that keeps the connection open after a complete reply
        // (Chrome's DevTools endpoint does, whatever `Connection: close`
        // asks) would otherwise leave this waiting for an end that never
        // comes, and the read timeout would turn a good reply into an error.
        // Found 24 Sep 2026 by two chats separately (Atlas's own browser
        // could never attach to Chrome); merged into this one check.
        if whole_reply(&out) {
            return Ok(out);
        }
    }
}

/// Has the whole reply arrived? A server may keep the connection open
/// whatever the request asked (Chrome's debugger endpoint does), so waiting
/// for it to close means waiting out the timeout and then failing with the
/// answer already in hand. The reply says its own length — `Content-Length`,
/// or the zero-size last chunk of a chunked body — and that's when it's done.
pub fn whole_reply(raw: &[u8]) -> bool {
    let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") else { return false };
    let head = String::from_utf8_lossy(&raw[..end]).to_ascii_lowercase();
    let body = &raw[end + 4..];
    for line in head.lines() {
        if let Some(v) = line.strip_prefix("content-length:") {
            return v.trim().parse::<usize>().map(|n| body.len() >= n).unwrap_or(false);
        }
        if line.starts_with("transfer-encoding:") && line.contains("chunked") {
            // The last chunk is a line holding only "0". Matched with the
            // line break before it, so a chunk whose data happens to end in
            // "...0" followed by a blank line isn't taken for the end.
            return body.ends_with(b"\r\n0\r\n\r\n") || body.starts_with(b"0\r\n\r\n");
        }
    }
    false
}

pub fn build_request(method: &str, host: &str, path: &str, body: Option<&str>) -> String {
    let mut r = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nAccept: */*\r\n"
    );
    match body {
        Some(b) => {
            r.push_str("Content-Type: application/json\r\n");
            r.push_str(&format!("Content-Length: {}\r\n\r\n", b.len()));
            r.push_str(b);
        }
        None => r.push_str("\r\n"),
    }
    r
}

/// Split headers from body and read the status. Chunked encoding is decoded
/// because Chrome uses it for some endpoints.
///
/// Takes bytes, and there is deliberately no `&str` sibling.
///
/// There was one — `parse_response(&str)` — and it is what the bug was made
/// of: `with_header` called it as
/// `parse_response(&String::from_utf8_lossy(&raw))`, so the lossy conversion
/// ran before the chunk offsets were used and every byte that would not
/// decode became three. Keeping it "for callers that already have text" would
/// leave that mistake one call away, and its only remaining callers were
/// tests. `http.rs`'s own comment above `with_header` says why that is not a
/// reason to keep a function: *"a third public entry point whose only caller
/// is in this file is a capability that exists to be counted rather than
/// used."*
///
/// A test holding text passes `s.as_bytes()`, which is the same thing said
/// out loud.
pub fn parse_response(raw: &[u8]) -> Result<Response> {
    let split = find(raw, 0, b"\r\n\r\n")
        .ok_or_else(|| AtlasError::Platform("malformed HTTP response".into()))?;
    let head_bytes = &raw[..split];
    let body = &raw[split + 4..];

    // Headers are ASCII by definition, so decoding them cannot move an
    // offset that matters.
    let head = String::from_utf8_lossy(head_bytes);

    let status = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse::<u16>().ok())
        .ok_or_else(|| AtlasError::Platform("no HTTP status line".into()))?;

    let chunked = head
        .to_lowercase()
        .contains("transfer-encoding: chunked");

    let location = head
        .lines()
        .find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case("location")).map(|(_, v)| v.trim().to_string()));

    Ok(Response {
        status,
        location,
        body: if chunked {
            dechunk(body)
        } else {
            String::from_utf8_lossy(body).into_owned()
        },
    })
}

/// Exposed for tests only — the decoder is otherwise private.
#[doc(hidden)]
pub fn dechunk_for_test(body: &str) -> String {
    dechunk(body.as_bytes())
}

fn dechunk(src: &[u8]) -> String {
    // Bytes, not characters. A chunk size on the wire counts bytes, and a
    // server splits wherever its buffer ran out — which is routinely in the
    // middle of a character. Slicing a `&str` at that offset panics, so a
    // page with an accent or a curly quote in the wrong place took Atlas down
    // rather than returning a page.
    //
    // Reassembling first and decoding once at the end is also the only way to
    // get the right answer: a character split across two chunks is only valid
    // when the chunks are back together.
    let mut out: Vec<u8> = Vec::with_capacity(src.len());
    let mut i = 0usize;

    while let Some(eol) = find(src, i, b"\r\n") {
        // The size line runs to the next CRLF.
        let line = &src[i..eol];
        let text = String::from_utf8_lossy(line);
        let size_part = text.trim().split(';').next().unwrap_or("").trim();
        let Ok(n) = usize::from_str_radix(size_part, 16) else { break };

        let start = eol + 2;
        if n == 0 {
            break;
        }
        // A chunk that promised more than arrived is an incomplete chunk, and
        // the whole complete chunks before it are the part that can be trusted.
        // `a_truncated_chunked_body_yields_what_arrived_rather_than_panicking`
        // fixes this deliberately: including the partial tail would hand back
        // a half-page indistinguishable from a whole one, and you cannot tell
        // from the outside how much is missing.
        if start + n > src.len() {
            break;
        }
        out.extend_from_slice(&src[start..start + n]);
        i = start + n;
        // Skip the CRLF that follows the chunk data.
        if src[i..].starts_with(b"\r\n") {
            i += 2;
        }
    }

    // Decoded once, at the end, with anything malformed replaced rather than
    // refused. A page that is 99% readable is worth more than an error.
    String::from_utf8_lossy(&out).into_owned()
}

/// First occurrence of `needle` in `hay` at or after `from`.
fn find(hay: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if from >= hay.len() {
        return None;
    }
    hay[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}
