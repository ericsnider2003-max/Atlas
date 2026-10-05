//! IMAP, from scratch, over whatever transport you hand it.
//!
//! Not curl. curl's IMAP support is real but incomplete — its own
//! maintainers describe `UID FETCH` as unimplemented and custom commands
//! as "overloading the behaviour of a LIST command." Good enough for a
//! single URL-shaped fetch (which is all `research.rs` needs), not good
//! enough for a real mail client that has to `SEARCH`, then `UID FETCH`
//! exactly the messages that search found.
//!
//! `Session<S>` is generic over `S: Read + Write` rather than hardcoding a
//! TLS socket, on purpose: the protocol logic — command formatting,
//! response parsing, literal handling — is the part worth getting right
//! and the part a test can actually exercise, by handing it an in-memory
//! stream instead of a real server. The real transport (`native-tls` over
//! `TcpStream`) is one thin call site, not tangled into the parsing.

use std::io::{self, Read, Write};
use std::net::TcpStream;

/// A live connection, over real TLS. The one place this module touches an
/// actual socket — everything else is `Session<S>`, generic, and tested
/// without one.
///
/// Untested here, deliberately: there is no real IMAP server to connect
/// to from this machine, and testing it would mean either a live account
/// (not something to do from an automated run) or a fake TLS server,
/// which would only prove this code talks to itself. What's proven
/// instead is everything this depends on — the protocol logic above, and
/// `native_tls`'s own correctness, which is not this module's job to
/// re-verify. The first real connection has to happen on a machine that
/// can actually reach `imap.gmail.com`.
pub fn connect(host: &str, port: u16) -> Result<Session<native_tls::TlsStream<TcpStream>>, String> {
    let tcp =
        TcpStream::connect((host, port)).map_err(|e| format!("couldn't reach {host}:{port}: {e}"))?;
    // Before TLS, so the handshake is covered too. A server that completes
    // the TCP connection and then goes quiet during the handshake used to
    // hang here with nothing to break it.
    tcp.set_read_timeout(Some(QUIET_FOR))
        .map_err(|e| format!("couldn't set a read timeout on {host}: {e}"))?;
    tcp.set_write_timeout(Some(QUIET_FOR))
        .map_err(|e| format!("couldn't set a write timeout on {host}: {e}"))?;
    let connector = native_tls::TlsConnector::new().map_err(|e| format!("couldn't set up TLS: {e}"))?;
    let tls =
        connector.connect(host, tcp).map_err(|e| format!("TLS handshake with {host} failed: {e}"))?;
    let mut session = Session::new(tls);
    let greeting = session.read_greeting().map_err(|e| format!("no greeting from {host}: {e}"))?;
    if !greeting.contains("OK") {
        return Err(format!("{host} did not say it was ready: {greeting}"));
    }
    Ok(session)
}

/// Where the money is: RFC 3501's `literal` is `{N}\r\n` followed by
/// exactly `N` bytes, which may contain CRLF themselves — the entire
/// reason literals exist is to carry text and binary data that IMAP's own
/// line-oriented framing would otherwise cut short. A parser that just
/// reads lines gets this wrong the moment a subject line or a message
/// body contains a bare `\r\n`.
pub struct Session<S: Read + Write> {
    stream: S,
    inbuf: Vec<u8>,
    tag: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Ok,
    No(String),
    Bad(String),
}

#[derive(Debug, Clone)]
pub struct Response {
    /// Every untagged (`* ...`) line, literals already substituted in.
    pub untagged: Vec<String>,
    pub status: Status,
}

impl Response {
    pub fn ok(&self) -> Result<(), String> {
        match &self.status {
            Status::Ok => Ok(()),
            Status::No(m) | Status::Bad(m) => Err(m.clone()),
        }
    }
}

/// One fetched message: just enough parsed out to categorize and extract
/// from — the headers a categorizer actually reads, plus the body text.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Message {
    pub uid: u32,
    pub from: String,
    pub subject: String,
    pub date: String,
    pub to: String,
    pub cc: String,
    pub in_reply_to: String,
    /// `Message-ID`, and `References` (every ancestor, oldest first). Asked
    /// for since 23 Sep so mail can be grouped into conversations
    /// (`mailthread`, the JWZ algorithm): In-Reply-To alone names only the
    /// parent, and a thread with one message missing from the window falls
    /// apart without the rest of the chain.
    pub message_id: String,
    pub references: String,
    /// `List-Unsubscribe`, when present — RFC 2369/8058. The one header
    /// that matters for a later unsubscribe feature; parsed here because
    /// this is the one place headers are already being split out, not
    /// because anything acts on it yet.
    pub list_unsubscribe: String,
    /// `Authentication-Results` (RFC 8601) — the receiving server's own
    /// verdict on SPF, DKIM and DMARC. The topmost one only: it is the one
    /// your provider added; any below it came with the message and could
    /// say anything. Read by `lookalike`.
    pub authentication_results: String,
    /// Has the `\Answered` flag. IMAP's own record of whether you've
    /// replied, not something to infer from the message itself.
    pub answered: bool,
    /// Has the `\Seen` flag — whether you've opened it. Together with
    /// `answered`, this is what turns a window of fetched mail into real
    /// per-sender engagement stats without Atlas having to keep its own
    /// separate history: the server already remembers what you did with
    /// each message.
    pub seen: bool,
    pub body: String,
}

/// The most either mail module will hold from a server before giving up.
///
/// 64 MiB: comfortably past a large message with attachments, and far short
/// of "until the machine stops". See `Session::fill`.
pub(crate) const MAX_BUFFER: usize = 64 * 1024 * 1024;

/// How long a mail socket may be silent before Atlas gives up on it.
///
/// There were no timeouts at all. A server that completes the TCP and TLS
/// handshakes and then stops talking hung `fill` for ever — and mail runs as
/// a crew errand, so the effect was a permanently stuck errand and a leaked
/// thread, once per mail check, accumulating for as long as Atlas ran.
/// `http.rs` sets both timeouts and has since it was written; these two were
/// never given the same.
pub(crate) const QUIET_FOR: std::time::Duration = std::time::Duration::from_secs(30);

/// One bounded read off a mail socket into `inbuf` -- the one place either
/// mail module (this and `smtp`) takes bytes off the wire (see `Session::fill`
/// for why the cap lives here). Shared, so the two can't drift apart again:
/// until 5 Oct 2026 each module carried its own copy of this, word for word.
pub(crate) fn fill_bounded<S: Read>(stream: &mut S, inbuf: &mut Vec<u8>) -> io::Result<()> {
    if inbuf.len() >= MAX_BUFFER {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "the server has sent {} bytes without finishing what it was \
                 saying, past the {MAX_BUFFER} I will hold. Stopping rather \
                 than filling memory.",
                inbuf.len()
            ),
        ));
    }
    let mut chunk = [0u8; 4096];
    let n = stream.read(&mut chunk)?;
    if n == 0 {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "server closed the connection"));
    }
    inbuf.extend_from_slice(&chunk[..n]);
    Ok(())
}

impl<S: Read + Write> Session<S> {
    pub fn new(stream: S) -> Session<S> {
        Session { stream, inbuf: Vec::new(), tag: 0 }
    }

    /// One read into the buffer, with the buffer bounded.
    ///
    /// ## The cap, and why it lives here
    ///
    /// `fill` is the only place either mail module takes bytes off the wire,
    /// so it is the one place a bound can cover every reader at once —
    /// `read_line`, `read_reply` and `read_exact_n` all loop on it and all
    /// used to loop without limit:
    ///
    /// * `read_line` grew the buffer until it saw CRLF. A server that never
    ///   sends one grows it for ever.
    /// * `read_reply` pushed every line shorter than four bytes and carried
    ///   on. A server streaming `"ok\r\n"` grows the line list for ever.
    /// * `read_exact_n(n)` took `n` straight from the server's own `{N}`
    ///   literal announcement, with no ceiling. A server announcing
    ///   `{4294967295}` makes Atlas buffer until the OOM killer arrives —
    ///   and the process it kills is the one holding the vault and the mail
    ///   credentials.
    ///
    /// None of those needs a hostile server; a broken one does it too. The
    /// cap is generous for the job — a large message with attachments is
    /// megabytes, not tens — and it fails with a sentence rather than by
    /// dying.
    fn fill(&mut self) -> io::Result<()> {
        fill_bounded(&mut self.stream, &mut self.inbuf)
    }

    /// One CRLF-terminated line, the CRLF itself dropped.
    fn read_line(&mut self) -> io::Result<Vec<u8>> {
        loop {
            if let Some(pos) = find_crlf(&self.inbuf) {
                let line: Vec<u8> = self.inbuf.drain(..pos).collect();
                self.inbuf.drain(..2);
                return Ok(line);
            }
            self.fill()?;
        }
    }

    /// Exactly `n` bytes, where `n` came from the server.
    ///
    /// The cap is explicit as well as covered by `fill`'s, because the
    /// failure reads very differently: `{4294967295}` is a server telling
    /// Atlas to expect four gigabytes, and refusing the *announcement* names
    /// the cause, where refusing the buffer halfway names a symptom.
    fn read_exact_n(&mut self, n: usize) -> io::Result<Vec<u8>> {
        if n > MAX_BUFFER {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "the server announced a {n}-byte literal, past the \
                     {MAX_BUFFER} I will hold for one. Refusing rather than \
                     buffering it."
                ),
            ));
        }
        while self.inbuf.len() < n {
            self.fill()?;
        }
        Ok(self.inbuf.drain(..n).collect())
    }

    fn next_tag(&mut self) -> String {
        self.tag += 1;
        format!("A{:04}", self.tag)
    }

    /// Read the server's opening greeting. Called once, right after
    /// connecting, before any command is sent — the greeting has no tag
    /// of its own to wait for.
    pub fn read_greeting(&mut self) -> io::Result<String> {
        let line = self.read_line()?;
        Ok(String::from_utf8_lossy(&line).into_owned())
    }

    /// Send one command, read every line up to and including its tagged
    /// completion, and hand back the untagged lines plus the status.
    pub fn command(&mut self, cmd: &str) -> io::Result<Response> {
        let tag = self.next_tag();
        let line = format!("{tag} {cmd}\r\n");
        self.stream.write_all(line.as_bytes())?;
        self.stream.flush()?;

        let mut untagged = Vec::new();
        loop {
            let mut text = self.read_line()?;
            if let Some(n) = literal_len(&text) {
                let body = self.read_exact_n(n)?;
                let rest = self.read_line()?;
                text.extend_from_slice(&body);
                text.extend_from_slice(&rest);
            }
            let line = String::from_utf8_lossy(&text).into_owned();
            let prefix = format!("{tag} ");
            if let Some(after) = line.strip_prefix(&prefix) {
                let status = if after.starts_with("OK") {
                    Status::Ok
                } else if let Some(m) = after.strip_prefix("NO ") {
                    Status::No(m.to_string())
                } else if let Some(m) = after.strip_prefix("BAD ") {
                    Status::Bad(m.to_string())
                } else {
                    Status::Bad(after.to_string())
                };
                return Ok(Response { untagged, status });
            }
            untagged.push(line);
        }
    }

    /// `LOGIN user pass`. IMAP's own quoting: backslash and double-quote
    /// inside either value must be escaped, or a password containing a
    /// quote mark breaks the command line for reasons that have nothing
    /// to do with authentication failing.
    pub fn login(&mut self, user: &str, pass: &str) -> Result<(), String> {
        let cmd = format!("LOGIN {} {}", quoted(user), quoted(pass));
        self.command(&cmd).map_err(|e| e.to_string())?.ok()
    }

    /// `AUTHENTICATE XOAUTH2` — Outlook/Microsoft 365's replacement for
    /// `LOGIN` now that password-based auth is gone entirely. Needs its
    /// own handling, not just a call to `command`: on failure the server
    /// doesn't just reply `NO`, it sends a `+` continuation carrying a
    /// base64 JSON error first, and the exchange isn't over until the
    /// client answers that continuation with an empty line. `command`
    /// has no notion of a continuation — it only knows how to wait for a
    /// tagged completion — so this reads the same way by hand.
    pub fn auth_xoauth2(&mut self, user: &str, access_token: &str) -> Result<(), String> {
        let initial = crate::msoauth::xoauth2_string(user, access_token);
        let tag = self.next_tag();
        let line = format!("{tag} AUTHENTICATE XOAUTH2 {initial}\r\n");
        self.stream.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
        self.stream.flush().map_err(|e| e.to_string())?;

        loop {
            let mut text = self.read_line().map_err(|e| e.to_string())?;
            if let Some(n) = literal_len(&text) {
                let body = self.read_exact_n(n).map_err(|e| e.to_string())?;
                let rest = self.read_line().map_err(|e| e.to_string())?;
                text.extend_from_slice(&body);
                text.extend_from_slice(&rest);
            }
            let line = String::from_utf8_lossy(&text).into_owned();
            if line.starts_with('+') {
                // The server reported a SASL-level error and is waiting
                // for an empty response to close out the exchange before
                // it will send the real tagged failure.
                self.stream.write_all(b"\r\n").map_err(|e| e.to_string())?;
                self.stream.flush().map_err(|e| e.to_string())?;
                continue;
            }
            let prefix = format!("{tag} ");
            if let Some(after) = line.strip_prefix(&prefix) {
                return if after.starts_with("OK") { Ok(()) } else { Err(after.to_string()) };
            }
            // An untagged line during authentication -- keep reading.
        }
    }

    pub fn select(&mut self, mailbox: &str) -> Result<(), String> {
        let cmd = format!("SELECT {}", quoted(mailbox));
        self.command(&cmd).map_err(|e| e.to_string())?.ok()
    }

    /// `UID SEARCH <criteria>`, returning the UIDs found. Always searched
    /// by UID rather than sequence number — sequence numbers shift when
    /// other messages are deleted, UIDs don't, and a categorizer that
    /// fetches the wrong message because something else changed the
    /// mailbox in between is a worse bug than a slower search.
    pub fn uid_search(&mut self, criteria: &str) -> Result<Vec<u32>, String> {
        let cmd = format!("UID SEARCH {criteria}");
        let resp = self.command(&cmd).map_err(|e| e.to_string())?;
        resp.ok()?;
        let mut uids = Vec::new();
        for line in &resp.untagged {
            let rest = line.strip_prefix("* SEARCH").or_else(|| line.strip_prefix("SEARCH"));
            if let Some(rest) = rest {
                for tok in rest.split_whitespace() {
                    if let Ok(n) = tok.parse::<u32>() {
                        uids.push(n);
                    }
                }
            }
        }
        Ok(uids)
    }

    /// `UID FETCH <uid> (...)`, parsed into a `Message`. One UID at a
    /// time — a range fetch is a real optimisation worth having later,
    /// but parsing multiple messages' literals out of one response
    /// correctly is meaningfully more code, and nothing here needs the
    /// speed yet.
    pub fn uid_fetch(&mut self, uid: u32) -> Result<Message, String> {
        let cmd = format!(
            "UID FETCH {uid} (FLAGS BODY.PEEK[HEADER.FIELDS (FROM SUBJECT DATE TO CC IN-REPLY-TO MESSAGE-ID REFERENCES LIST-UNSUBSCRIBE AUTHENTICATION-RESULTS)] BODY.PEEK[TEXT])"
        );
        let resp = self.command(&cmd).map_err(|e| e.to_string())?;
        resp.ok()?;
        Ok(parse_fetch(uid, &resp.untagged))
    }

    /// Where your sent mail is: the mailbox the server marks `\Sent`
    /// (RFC 6154 special-use), or failing that the usual names. Asked once
    /// per check, so the waiting-for list can see what you wrote.
    pub fn sent_mailbox(&mut self) -> Result<String, String> {
        let resp = self.command("LIST \"\" \"*\"").map_err(|e| e.to_string())?;
        resp.ok()?;
        let names: Vec<(bool, String)> = resp.untagged.iter().filter_map(|l| list_entry(l)).collect();
        if let Some((_, n)) = names.iter().find(|(sent, _)| *sent) {
            return Ok(n.clone());
        }
        for guess in ["[Gmail]/Sent Mail", "Sent Items", "Sent", "INBOX.Sent", "Sent Messages"] {
            if let Some((_, n)) = names.iter().find(|(_, n)| n.eq_ignore_ascii_case(guess)) {
                return Ok(n.clone());
            }
        }
        Err("I couldn't find your Sent folder".into())
    }

    /// Make a folder if it isn't there. "Already exists" is fine.
    pub fn create(&mut self, mailbox: &str) -> Result<(), String> {
        let resp = self.command(&format!("CREATE {}", quoted(mailbox))).map_err(|e| e.to_string())?;
        match resp.ok() {
            Ok(()) => Ok(()),
            Err(e) if e.to_uppercase().contains("ALREADYEXISTS") || e.to_lowercase().contains("exist") => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Gmail: add a label (`X-GM-LABELS`), leaving the message where it is.
    fn uid_label(&mut self, uid: u32, label: &str) -> Result<(), String> {
        self.command(&format!("UID STORE {uid} +X-GM-LABELS ({})", quoted(label)))
            .map_err(|e| e.to_string())?
            .ok()
    }

    /// Move a message to another folder (`UID MOVE`, RFC 6851).
    fn uid_move(&mut self, uid: u32, mailbox: &str) -> Result<(), String> {
        self.command(&format!("UID MOVE {uid} {}", quoted(mailbox)))
            .map_err(|e| e.to_string())?
            .ok()
    }

    fn uid_mark_read(&mut self, uid: u32) -> Result<(), String> {
        self.command(&format!("UID STORE {uid} +FLAGS (\\Seen)")).map_err(|e| e.to_string())?.ok()
    }

    /// Carry out one of `mail`'s actions on a message in the selected
    /// mailbox. Archive on Gmail is taking the Inbox label off; elsewhere it's
    /// a move to the Archive folder. There is no delete here.
    pub fn apply(&mut self, uid: u32, action: &crate::mail::Action, gmail: bool) -> Result<(), String> {
        use crate::mail::Action;
        match action {
            Action::Nothing => Ok(()),
            Action::MarkRead => self.uid_mark_read(uid),
            Action::Label(l) => self.uid_label(uid, l),
            Action::MoveTo(f) => {
                self.create(f)?;
                self.uid_move(uid, f)
            }
            Action::Archive if gmail => self
                .command(&format!("UID STORE {uid} -X-GM-LABELS (\\Inbox)"))
                .map_err(|e| e.to_string())?
                .ok(),
            Action::Archive => {
                self.create("Archive")?;
                self.uid_move(uid, "Archive")
            }
        }
    }

    /// The connection back, for a test to read what was sent.
    pub fn into_inner(self) -> S {
        self.stream
    }

    pub fn logout(&mut self) {
        let _ = self.command("LOGOUT");
    }

    /// Select a mailbox and fetch every message a search finds, in one
    /// call — the shape every real caller actually wants, rather than
    /// composing `select`/`uid_search`/`uid_fetch` by hand at each call
    /// site. One IMAP session, one mailbox, one search.
    pub fn fetch_matching(&mut self, mailbox: &str, criteria: &str) -> Result<Vec<Message>, String> {
        self.select(mailbox)?;
        let uids = self.uid_search(criteria)?;
        let mut out = Vec::with_capacity(uids.len());
        for uid in uids {
            out.push(self.uid_fetch(uid)?);
        }
        Ok(out)
    }
}

/// One `* LIST (\\HasNoChildren \\Sent) "/" "Sent"` line: (is it marked
/// \Sent, the mailbox name).
pub fn list_entry(line: &str) -> Option<(bool, String)> {
    let rest = line.strip_prefix("* LIST ")?;
    let close = rest.find(')')?;
    let flags = rest[..close].trim_start_matches('(').to_ascii_lowercase();
    let after = rest[close + 1..].trim_start();
    // The delimiter: a quoted character or NIL.
    let after = if let Some(a) = after.strip_prefix("NIL") { a } else {
        let a = after.strip_prefix('"')?;
        let end = a.find('"')?;
        &a[end + 1..]
    };
    let name = after.trim();
    let name = name.strip_prefix('"').and_then(|n| n.strip_suffix('"')).unwrap_or(name).to_string();
    (!name.is_empty()).then(|| (flags.split_whitespace().any(|f| f == "\\sent"), name))
}

fn find_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\r\n")
}

/// A line ending in `{N}` (optionally with `+` for non-synchronizing
/// literals, which this treats the same way — the server still sends the
/// bytes either way) means the next N bytes are a literal, not text to
/// interpret as IMAP syntax.
fn literal_len(line: &[u8]) -> Option<usize> {
    let s = std::str::from_utf8(line).ok()?;
    let s = s.trim_end();
    let close = s.rfind('}')?;
    let open = s[..close].rfind('{')?;
    let digits = s[open + 1..close].trim_end_matches('+');
    digits.parse::<usize>().ok()
}

fn quoted(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// Pull the headers and body text back out of a `FETCH` response's
/// untagged lines, once literals have already been substituted in by
/// `Session::command`.
fn parse_fetch(uid: u32, lines: &[String]) -> Message {
    let joined = lines.join("\n");
    let mut m = Message { uid, ..Default::default() };
    m.answered = flags_contain(&joined, "\\Answered");
    m.seen = flags_contain(&joined, "\\Seen");
    if let Some(marker_pos) = joined.find("HEADER.FIELDS") {
        if let Some(brace_close) = joined[marker_pos..].find('}') {
            let header_start = marker_pos + brace_close + 1;
            let header_block = &joined[header_start..];
            // Unfolded first (RFC 5322 §2.2.3): a long `References:` is
            // routinely split over several lines, each continuation starting
            // with whitespace, and reading it line by line would keep only
            // the first few ids.
            let mut unfolded: Vec<String> = Vec::new();
            for line in header_block.lines() {
                if line.trim().is_empty() {
                    unfolded.push(String::new());
                    break;
                }
                match unfolded.last_mut() {
                    Some(prev) if line.starts_with(' ') || line.starts_with('\t') => {
                        prev.push(' ');
                        prev.push_str(line.trim());
                    }
                    _ => unfolded.push(line.to_string()),
                }
            }
            for line in unfolded.iter().map(|l| l.as_str()) {
                let lower = line.to_ascii_lowercase();
                if let Some(v) = line.strip_prefix_ci(&lower, "from:") {
                    m.from = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix_ci(&lower, "subject:") {
                    m.subject = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix_ci(&lower, "date:") {
                    m.date = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix_ci(&lower, "to:") {
                    m.to = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix_ci(&lower, "cc:") {
                    m.cc = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix_ci(&lower, "in-reply-to:") {
                    m.in_reply_to = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix_ci(&lower, "message-id:") {
                    m.message_id = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix_ci(&lower, "references:") {
                    m.references = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix_ci(&lower, "list-unsubscribe:") {
                    m.list_unsubscribe = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix_ci(&lower, "authentication-results:") {
                    if m.authentication_results.is_empty() {
                        m.authentication_results = v.trim().to_string();
                    }
                } else if line.trim().is_empty() {
                    break;
                }
            }
        }
    }
    if let Some(text_start) = joined.rfind("BODY[TEXT]") {
        // Everything after the section marker up to the closing `)` that
        // ends the FETCH response is the body literal.
        let tail = &joined[text_start..];
        if let Some(brace_end) = tail.find('}') {
            let after = &tail[brace_end + 1..];
            let body = after.strip_suffix(")").unwrap_or(after);
            m.body = body.trim_start_matches('\n').to_string();
        }
    }
    m
}

/// Whether the `FLAGS (...)` part of a FETCH response names a given flag.
/// A plain substring search rather than parsing the parenthesised list
/// properly — the flag names IMAP defines (`\Answered`, `\Seen`, ...) are
/// backslash-prefixed and space-separated, so a bare substring match
/// can't accidentally match part of a different flag or a header value
/// that happens to contain the same letters.
fn flags_contain(joined: &str, flag: &str) -> bool {
    if let Some(start) = joined.find("FLAGS (") {
        if let Some(end) = joined[start..].find(')') {
            return joined[start..start + end].contains(flag);
        }
    }
    false
}

/// Small helper so the header-matching above reads as one line each
/// rather than a `to_ascii_lowercase` at every branch.
trait StripPrefixCi {
    fn strip_prefix_ci<'a>(&'a self, lower_self: &str, prefix: &str) -> Option<&'a str>;
}
impl StripPrefixCi for str {
    fn strip_prefix_ci<'a>(&'a self, lower_self: &str, prefix: &str) -> Option<&'a str> {
        if lower_self.starts_with(prefix) {
            Some(&self[prefix.len()..])
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// A fake server: replies come from a fixed script, writes go nowhere
    /// (or are captured for inspection). This is the whole point of
    /// making `Session` generic — none of this needs a socket.
    struct Scripted {
        replies: Cursor<Vec<u8>>,
        sent: Vec<u8>,
    }
    impl Scripted {
        fn new(script: &str) -> Scripted {
            Scripted { replies: Cursor::new(script.as_bytes().to_vec()), sent: Vec::new() }
        }
    }
    impl Read for Scripted {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.replies.read(buf)
        }
    }
    impl Write for Scripted {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.sent.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_plain_ok_completion_is_read_correctly() {
        let s = Scripted::new("A0001 OK LOGIN completed\r\n");
        let mut sess = Session::new(s);
        let r = sess.command("LOGIN x x").unwrap();
        assert!(r.ok().is_ok());
        assert!(r.untagged.is_empty());
    }

    #[test]
    fn a_no_completion_carries_the_servers_own_reason() {
        let s = Scripted::new("A0001 NO [AUTHENTICATIONFAILED] Invalid credentials\r\n");
        let mut sess = Session::new(s);
        let r = sess.command("LOGIN x x").unwrap();
        let err = r.ok().unwrap_err();
        assert!(err.contains("Invalid credentials"), "got: {err}");
    }

    #[test]
    fn search_pulls_every_uid_out_of_the_untagged_line() {
        let s = Scripted::new("* SEARCH 4 19 302\r\nA0001 OK SEARCH completed\r\n");
        let mut sess = Session::new(s);
        let uids = sess.uid_search("ALL").unwrap();
        assert_eq!(uids, vec![4, 19, 302]);
    }

    #[test]
    fn search_with_no_hits_is_empty_not_an_error() {
        let s = Scripted::new("* SEARCH\r\nA0001 OK SEARCH completed\r\n");
        let mut sess = Session::new(s);
        let uids = sess.uid_search("ALL").unwrap();
        assert!(uids.is_empty());
    }

    #[test]
    fn a_literal_body_containing_a_bare_crlf_is_read_whole_not_cut_at_the_first_line() {
        // The entire reason literals exist: this body contains a bare
        // CRLF, which a naive line reader would mistake for the end of
        // the response. `{18}` is the exact byte length of the body text
        // below, CRLF included.
        let body = "line one\r\nline two";
        assert_eq!(body.len(), 18);
        let script = format!(
            "* 7 FETCH (UID 7 BODY[TEXT] {{{}}}\r\n{}\r\n)\r\nA0001 OK FETCH completed\r\n",
            body.len(),
            body
        );
        let s = Scripted::new(&script);
        let mut sess = Session::new(s);
        let resp = sess.command("UID FETCH 7 (BODY[TEXT])").unwrap();
        resp.ok().unwrap();
        let joined = resp.untagged.join("\n");
        assert!(joined.contains("line one"), "got: {joined}");
        assert!(joined.contains("line two"), "got: {joined}");
    }

    #[test]
    fn login_quotes_a_password_containing_a_double_quote() {
        let s = Scripted::new("A0001 OK LOGIN completed\r\n");
        let mut sess = Session::new(s);
        sess.login("me@example.com", "pa\"ss").unwrap();
        let sent = String::from_utf8_lossy(&sess.stream.sent).into_owned();
        assert!(sent.contains("\\\"ss"), "the quote wasn't escaped: {sent}");
    }

    #[test]
    fn login_quotes_a_password_containing_a_backslash() {
        let s = Scripted::new("A0001 OK LOGIN completed\r\n");
        let mut sess = Session::new(s);
        sess.login("me@example.com", "back\\slash").unwrap();
        let sent = String::from_utf8_lossy(&sess.stream.sent).into_owned();
        assert!(sent.contains("back\\\\slash"), "the backslash wasn't escaped: {sent}");
    }

    #[test]
    fn each_command_gets_a_fresh_tag_so_responses_cannot_be_mismatched() {
        let s = Scripted::new(
            "A0001 OK LOGIN completed\r\nA0002 OK SELECT completed\r\n",
        );
        let mut sess = Session::new(s);
        sess.login("a", "b").unwrap();
        // If the tag didn't advance, this would hang waiting for a line
        // starting with "A0001 " that will never come again.
        sess.select("INBOX").unwrap();
    }

    #[test]
    fn a_fetch_response_is_parsed_into_a_real_message() {
        let header =
            "From: alerts@bank.example\r\nSubject: Your statement is ready\r\nDate: Mon, 1 Jan 2026 09:00:00 -0800\r\n\r\n";
        let body = "Your statement is ready to view.";
        let script = format!(
            "* 3 FETCH (UID 3 BODY[HEADER.FIELDS (FROM SUBJECT DATE LIST-UNSUBSCRIBE)] {{{}}}\r\n{}\r\n BODY[TEXT] {{{}}}\r\n{})\r\nA0001 OK FETCH completed\r\n",
            header.len(),
            header,
            body.len(),
            body
        );
        let s = Scripted::new(&script);
        let mut sess = Session::new(s);
        let msg = sess.uid_fetch(3).unwrap();
        assert_eq!(msg.uid, 3);
        assert_eq!(msg.from, "alerts@bank.example");
        assert_eq!(msg.subject, "Your statement is ready");
        assert!(msg.body.contains("statement is ready to view"));
    }

    #[test]
    fn list_unsubscribe_is_pulled_out_when_present() {
        let header = "From: news@shop.example\r\nSubject: Sale\r\nDate: Mon, 1 Jan 2026 09:00:00 -0800\r\nList-Unsubscribe: <mailto:unsub@shop.example>\r\n\r\n";
        let body = "hi";
        let script = format!(
            "* 9 FETCH (UID 9 BODY[HEADER.FIELDS (FROM SUBJECT DATE LIST-UNSUBSCRIBE)] {{{}}}\r\n{}\r\n BODY[TEXT] {{{}}}\r\n{})\r\nA0001 OK FETCH completed\r\n",
            header.len(),
            header,
            body.len(),
            body
        );
        let s = Scripted::new(&script);
        let mut sess = Session::new(s);
        let msg = sess.uid_fetch(9).unwrap();
        assert_eq!(msg.list_unsubscribe, "<mailto:unsub@shop.example>");
    }

    #[test]
    fn the_greeting_is_read_before_any_command_is_sent() {
        let s = Scripted::new("* OK IMAP4rev1 Service Ready\r\nA0001 OK LOGIN completed\r\n");
        let mut sess = Session::new(s);
        let greeting = sess.read_greeting().unwrap();
        assert!(greeting.contains("Service Ready"));
        sess.login("a", "b").unwrap();
    }

    #[test]
    fn the_answered_flag_is_read_from_the_flags_list() {
        let header = "From: a@example.com\r\nSubject: Re: thing\r\nDate: Mon, 1 Jan 2026 09:00:00 -0800\r\n\r\n";
        let body = "ok";
        let script = format!(
            "* 5 FETCH (UID 5 FLAGS (\\Seen \\Answered) BODY[HEADER.FIELDS (FROM SUBJECT DATE TO CC IN-REPLY-TO LIST-UNSUBSCRIBE)] {{{}}}\r\n{}\r\n BODY[TEXT] {{{}}}\r\n{})\r\nA0001 OK FETCH completed\r\n",
            header.len(),
            header,
            body.len(),
            body
        );
        let s = Scripted::new(&script);
        let mut sess = Session::new(s);
        let msg = sess.uid_fetch(5).unwrap();
        assert!(msg.answered, "the \\Answered flag was not recognised");
    }

    #[test]
    fn a_message_with_no_answered_flag_is_not_marked_answered() {
        let header = "From: a@example.com\r\nSubject: thing\r\nDate: Mon, 1 Jan 2026 09:00:00 -0800\r\n\r\n";
        let body = "ok";
        let script = format!(
            "* 6 FETCH (UID 6 FLAGS (\\Seen) BODY[HEADER.FIELDS (FROM SUBJECT DATE TO CC IN-REPLY-TO LIST-UNSUBSCRIBE)] {{{}}}\r\n{}\r\n BODY[TEXT] {{{}}}\r\n{})\r\nA0001 OK FETCH completed\r\n",
            header.len(),
            header,
            body.len(),
            body
        );
        let s = Scripted::new(&script);
        let mut sess = Session::new(s);
        let msg = sess.uid_fetch(6).unwrap();
        assert!(!msg.answered);
    }

    #[test]
    fn to_cc_and_in_reply_to_are_all_pulled_from_the_header_block() {
        let header = "From: a@example.com\r\nSubject: thing\r\nDate: Mon, 1 Jan 2026 09:00:00 -0800\r\nTo: me@example.com, someone-else@example.com\r\nCc: third@example.com\r\nIn-Reply-To: <abc123@example.com>\r\n\r\n";
        let body = "ok";
        let script = format!(
            "* 8 FETCH (UID 8 BODY[HEADER.FIELDS (FROM SUBJECT DATE TO CC IN-REPLY-TO LIST-UNSUBSCRIBE)] {{{}}}\r\n{}\r\n BODY[TEXT] {{{}}}\r\n{})\r\nA0001 OK FETCH completed\r\n",
            header.len(),
            header,
            body.len(),
            body
        );
        let s = Scripted::new(&script);
        let mut sess = Session::new(s);
        let msg = sess.uid_fetch(8).unwrap();
        assert!(msg.to.contains("someone-else@example.com"), "got: {}", msg.to);
        assert_eq!(msg.cc, "third@example.com");
        assert_eq!(msg.in_reply_to, "<abc123@example.com>");
    }

    #[test]
    fn fetch_matching_selects_searches_and_fetches_every_hit_in_one_call() {
        let header_a = "From: a@example.com\r\nSubject: First\r\nDate: Mon, 1 Jan 2026 09:00:00 -0800\r\n\r\n";
        let header_b = "From: b@example.com\r\nSubject: Second\r\nDate: Mon, 1 Jan 2026 10:00:00 -0800\r\n\r\n";
        let body = "hi";
        let script = format!(
            "A0001 OK SELECT completed\r\n\
             * SEARCH 1 2\r\nA0002 OK SEARCH completed\r\n\
             * 1 FETCH (UID 1 BODY[HEADER.FIELDS (FROM SUBJECT DATE TO CC IN-REPLY-TO LIST-UNSUBSCRIBE)] {{{}}}\r\n{}\r\n BODY[TEXT] {{{}}}\r\n{})\r\nA0003 OK FETCH completed\r\n\
             * 2 FETCH (UID 2 BODY[HEADER.FIELDS (FROM SUBJECT DATE TO CC IN-REPLY-TO LIST-UNSUBSCRIBE)] {{{}}}\r\n{}\r\n BODY[TEXT] {{{}}}\r\n{})\r\nA0004 OK FETCH completed\r\n",
            header_a.len(), header_a, body.len(), body,
            header_b.len(), header_b, body.len(), body,
        );
        let s = Scripted::new(&script);
        let mut sess = Session::new(s);
        let msgs = sess.fetch_matching("INBOX", "ALL").unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].subject, "First");
        assert_eq!(msgs[1].subject, "Second");
    }

    #[test]
    fn fetch_matching_with_no_hits_returns_an_empty_list_not_an_error() {
        let script = "A0001 OK SELECT completed\r\n* SEARCH\r\nA0002 OK SEARCH completed\r\n";
        let s = Scripted::new(script);
        let mut sess = Session::new(s);
        let msgs = sess.fetch_matching("INBOX", "ALL").unwrap();
        assert!(msgs.is_empty());
    }

    #[test]
    fn find_crlf_locates_the_first_crlf_pair_not_a_bare_lf_or_cr() {
        assert_eq!(find_crlf(b"abc\r\ndef"), Some(3));
        assert_eq!(find_crlf(b"no line ending here"), None);
        assert_eq!(find_crlf(b"has a bare \n newline but no crlf"), None);
        assert_eq!(find_crlf(b"has a bare \r return but no crlf"), None);
    }

    #[test]
    fn literal_len_reads_the_brace_delimited_byte_count() {
        assert_eq!(literal_len(b"BODY[TEXT] {123}"), Some(123));
        assert_eq!(literal_len(b"BODY[TEXT] {0}"), Some(0));
        // Non-synchronizing literals end in `+` before the closing brace.
        assert_eq!(literal_len(b"BODY[TEXT] {45+}"), Some(45));
        assert_eq!(literal_len(b"A0001 OK LOGIN completed"), None);
    }

    #[test]
    fn quoted_escapes_both_backslash_and_double_quote() {
        assert_eq!(quoted("plain"), "\"plain\"");
        assert_eq!(quoted("has\"quote"), "\"has\\\"quote\"");
        assert_eq!(quoted("has\\slash"), "\"has\\\\slash\"");
    }

    #[test]
    fn flags_contain_matches_a_flag_inside_the_parenthesised_list_only() {
        assert!(flags_contain("* 1 FETCH (FLAGS (\\Seen \\Answered) UID 1)", "\\Answered"));
        assert!(!flags_contain("* 1 FETCH (FLAGS (\\Seen) UID 1)", "\\Answered"));
        // No FLAGS section at all.
        assert!(!flags_contain("* 1 FETCH (UID 1)", "\\Answered"));
    }

    #[test]
    fn auth_xoauth2_succeeds_on_a_plain_ok() {
        let s = Scripted::new("A0001 OK AUTHENTICATE completed\r\n");
        let mut sess = Session::new(s);
        sess.auth_xoauth2("me@outlook.com", "sometoken").unwrap();
    }

    #[test]
    fn auth_xoauth2_answers_a_continuation_before_reading_the_real_failure() {
        // The server sends a `+` continuation carrying its own error
        // first, then waits for an empty line before it will send the
        // tagged failure. A client that doesn't answer the continuation
        // would hang here forever.
        let error_json = crate::b64::encode(br#"{"status":"400","schemes":"bearer"}"#);
        let script = format!("+ {error_json}\r\nA0001 NO SASL authentication failed\r\n");
        let s = Scripted::new(&script);
        let mut sess = Session::new(s);
        let err = sess.auth_xoauth2("me@outlook.com", "badtoken").unwrap_err();
        assert!(err.contains("SASL authentication failed"), "got: {err}");
        let sent = String::from_utf8_lossy(&sess.stream.sent).into_owned();
        assert!(sent.ends_with("\r\n\r\n"), "the continuation must be answered with an empty line");
    }

    #[test]
    fn auth_xoauth2_sends_the_initial_response_on_the_same_line_as_the_command() {
        let s = Scripted::new("A0001 OK AUTHENTICATE completed\r\n");
        let mut sess = Session::new(s);
        sess.auth_xoauth2("me@outlook.com", "sometoken").unwrap();
        let sent = String::from_utf8_lossy(&sess.stream.sent).into_owned();
        assert!(sent.starts_with("A0001 AUTHENTICATE XOAUTH2 "));
        let expected = crate::msoauth::xoauth2_string("me@outlook.com", "sometoken");
        assert!(sent.contains(&expected));
    }
}
