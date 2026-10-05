//! SMTP, from scratch, over whatever transport you hand it.
//!
//! The only two things Atlas is allowed to use this for: unsubscribing
//! (the one send that never needs asking — Eric's rule, not a default
//! this module invented) and, once given standing approval, mail to
//! clients or brands. Neither of those decisions lives here — this
//! module only knows how to *send*, not when it's allowed to.
//!
//! Same shape as `imap.rs` on purpose: `Session<S>` is generic over
//! `Read + Write` so the protocol — command formatting, multi-line reply
//! parsing — is testable against an in-memory stream, and the one real
//! socket (`connect`, over TLS) is a single, separately-untestable call
//! site rather than tangled into the parsing.

use std::io::{self, Read, Write};

pub struct Session<S: Read + Write> {
    stream: S,
    inbuf: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub code: u32,
    pub lines: Vec<String>,
}

impl Reply {
    /// SMTP's own convention: 2xx and 3xx are success (3xx meaning "go
    /// on", as in "354 start mail input"); 4xx and 5xx are failures.
    pub fn ok(&self) -> bool {
        self.code < 400
    }

    pub fn text(&self) -> String {
        self.lines.join(" ")
    }
}

// The buffer cap and the quiet timeout are the IMAP module's: one copy of
// each for both mail protocols (`imap::MAX_BUFFER`, `imap::QUIET_FOR`).
use crate::imap::QUIET_FOR;

impl<S: Read + Write> Session<S> {
    pub fn new(stream: S) -> Session<S> {
        Session { stream, inbuf: Vec::new() }
    }

    /// The plaintext half of STARTTLS (RFC 3207): EHLO, check the server
    /// offers it, ask, and hand back the stream ready for the TLS handshake.
    ///
    /// Anything the server sent after its 220 is refused rather than carried
    /// into the encrypted session: bytes read before TLS but answered after it
    /// are the STARTTLS command-injection hole (CVE-2011-0411 and kin).
    pub fn starttls(mut self, client_name: &str) -> Result<S, String> {
        let r = self.ehlo(client_name)?;
        if !r.lines.iter().any(|l| l.to_ascii_uppercase().starts_with("STARTTLS")) {
            return Err("the server doesn't offer STARTTLS, so nothing was sent over a plain connection".into());
        }
        let r = self.command("STARTTLS").map_err(|e| e.to_string())?;
        if r.code != 220 {
            return Err(format!("the server refused STARTTLS: {}", r.text()));
        }
        if !self.inbuf.is_empty() {
            return Err("the server sent data before encryption started; refusing it".into());
        }
        Ok(self.stream)
    }

    /// One bounded read into the buffer (`imap::fill_bounded`, shared).
    fn fill(&mut self) -> io::Result<()> {
        crate::imap::fill_bounded(&mut self.stream, &mut self.inbuf)
    }

    fn read_line(&mut self) -> io::Result<String> {
        loop {
            if let Some(pos) = self.inbuf.windows(2).position(|w| w == b"\r\n") {
                let line: Vec<u8> = self.inbuf.drain(..pos).collect();
                self.inbuf.drain(..2);
                return Ok(String::from_utf8_lossy(&line).into_owned());
            }
            self.fill()?;
        }
    }

    /// A reply is one or more lines sharing a code; every line but the
    /// last has a `-` right after the code (`250-STARTTLS`), the last has
    /// a space (`250 AUTH LOGIN PLAIN`). Reading stops at the first line
    /// without a dash.
    fn read_reply(&mut self) -> io::Result<Reply> {
        let mut lines = Vec::new();
        let code = loop {
            let line = self.read_line()?;
            if line.len() < 4 {
                lines.push(line);
                continue;
            }
            // Bytes, not a string slice.
            //
            // `line[..3]` byte-slices a `String` that came from
            // `from_utf8_lossy` -- valid UTF-8, but not necessarily ASCII. A
            // reply line beginning with two two-byte characters is four bytes
            // long, so it passes the guard above and then puts index 3 in the
            // middle of a character, and `&line[..3]` **panics**. Reached from
            // `read_greeting`, i.e. before authentication, from anything on
            // the wire that answers on 465.
            let bytes = line.as_bytes();
            let code = std::str::from_utf8(&bytes[..3]).ok().and_then(|c| c.parse().ok()).unwrap_or(0);
            let sep = bytes[3];
            // Same hazard one line down: `line[4..]` is a string slice at a
            // byte offset. Taken from the bytes and decoded, so a non-ASCII
            // reply is mangled rather than fatal.
            lines.push(String::from_utf8_lossy(&bytes[4..]).into_owned());
            if sep != b'-' {
                break code;
            }
        };
        Ok(Reply { code, lines })
    }

    pub fn read_greeting(&mut self) -> io::Result<Reply> {
        self.read_reply()
    }

    pub fn command(&mut self, cmd: &str) -> io::Result<Reply> {
        self.stream.write_all(cmd.as_bytes())?;
        self.stream.write_all(b"\r\n")?;
        self.stream.flush()?;
        self.read_reply()
    }

    pub fn ehlo(&mut self, client_name: &str) -> Result<Reply, String> {
        let r = self.command(&format!("EHLO {client_name}")).map_err(|e| e.to_string())?;
        if !r.ok() {
            return Err(r.text());
        }
        Ok(r)
    }

    /// `AUTH LOGIN` — username and password each sent as their own
    /// base64-encoded line, in response to the server's own `334`
    /// prompts. The prompts' own text ("VXNlcm5hbWU6", "UGFzc3dvcmQ6" —
    /// base64 for "Username:"/"Password:") isn't decoded or checked; the
    /// exchange is positional (username first, password second) per RFC
    /// 4954, not driven by what the prompt says.
    pub fn auth_login(&mut self, user: &str, pass: &str) -> Result<(), String> {
        let r = self.command("AUTH LOGIN").map_err(|e| e.to_string())?;
        if r.code != 334 {
            return Err(format!("server didn't offer AUTH LOGIN: {}", r.text()));
        }
        let r = self.command(&crate::b64::encode(user.as_bytes())).map_err(|e| e.to_string())?;
        if r.code != 334 {
            return Err(format!("username rejected: {}", r.text()));
        }
        let r = self.command(&crate::b64::encode(pass.as_bytes())).map_err(|e| e.to_string())?;
        if !r.ok() {
            return Err(format!("authentication failed: {}", r.text()));
        }
        Ok(())
    }

    /// `AUTH XOAUTH2` — Outlook/Microsoft 365's SMTP auth, once password
    /// auth is gone. Simpler than IMAP's version: SMTP's continuation is
    /// just an ordinary reply with code 334, not a special marker, so
    /// `command` already knows how to read it — this only has to notice
    /// the 334 and answer it with an empty line to get the real result.
    pub fn auth_xoauth2(&mut self, user: &str, access_token: &str) -> Result<(), String> {
        let initial = crate::msoauth::xoauth2_string(user, access_token);
        let r = self.command(&format!("AUTH XOAUTH2 {initial}")).map_err(|e| e.to_string())?;
        // 334 is checked before the general `ok()` — a continuation is
        // technically a "3xx, keep going" code, so `ok()` alone reads it
        // as success and never gets the chance to answer it.
        if r.code == 334 {
            let r2 = self.command("").map_err(|e| e.to_string())?;
            return Err(r2.text());
        }
        if r.ok() {
            return Ok(());
        }
        Err(r.text())
    }

    /// One message, start to finish: `MAIL FROM`, `RCPT TO`, the `DATA`
    /// block, terminated the way SMTP requires — a line that is just
    /// `.`, and any line in the body that itself starts with `.` gets a
    /// second `.` in front of it first, or the server reads it as the
    /// terminator instead of content. `subject`/`body` are ASCII-assumed;
    /// nothing here MIME-encodes non-ASCII text, which is a real
    /// limitation for names and subjects outside it.
    pub fn send_mail(&mut self, from: &str, to: &str, subject: &str, body: &str) -> Result<(), String> {
        self.send_mail_in(from, to, subject, body, &crate::outbox::Thread::default())
    }

    /// As `send_mail`, as a reply in a conversation (`message_text_in`).
    pub fn send_mail_in(&mut self, from: &str, to: &str, subject: &str, body: &str, thread: &crate::outbox::Thread) -> Result<(), String> {
        let (from, to) = (plain_address(from)?, plain_address(to)?);
        let r = self.command(&format!("MAIL FROM:<{from}>")).map_err(|e| e.to_string())?;
        if !r.ok() {
            return Err(format!("MAIL FROM refused: {}", r.text()));
        }
        let r = self.command(&format!("RCPT TO:<{to}>")).map_err(|e| e.to_string())?;
        if !r.ok() {
            return Err(format!("RCPT TO refused: {}", r.text()));
        }
        let r = self.command("DATA").map_err(|e| e.to_string())?;
        if r.code != 354 {
            return Err(format!("server refused to start the message: {}", r.text()));
        }
        let message = format!("{}\r\n.\r\n", escape_dot_stuffing(&if thread.in_reply_to.is_empty() {
            message_text(from, to, subject, body, crate::store::now())
        } else {
            message_text_in(from, to, subject, body, crate::store::now(), thread)
        }));
        self.stream.write_all(message.as_bytes()).map_err(|e| e.to_string())?;
        self.stream.flush().map_err(|e| e.to_string())?;
        let r = self.read_reply().map_err(|e| e.to_string())?;
        if !r.ok() {
            return Err(format!("message refused: {}", r.text()));
        }
        Ok(())
    }

    pub fn quit(&mut self) {
        let _ = self.command("QUIT");
    }
}

/// A whole message as mail servers expect it: a `Date` and `Message-ID`
/// (without them many providers score it as spam), the character set said
/// (a curly apostrophe or an accent arrived as mojibake), and a subject
/// that isn't plain ASCII encoded (RFC 2047). Lines end CRLF. 30 Sep 2026:
/// only From, To and Subject were written.
/// One bare address -- `name@example.com` -- or why it isn't one. Checked
/// before it goes into `RCPT TO:<…>` or a `To:` line, where a line break, a
/// bracket or a second address would be a second command or a second
/// recipient (1 Oct 2026 security pass: only the subject was cleaned).
pub fn plain_address(a: &str) -> Result<&str, String> {
    let a = a.trim();
    let bad = a.chars().any(|c| c.is_whitespace() || c.is_control() || "<>()[],;:\\\"".contains(c));
    match a.split_once('@') {
        Some((user, host)) if !bad && !user.is_empty() && host.contains('.') && !host.contains('@') && !host.starts_with('.') && !host.ends_with('.') => Ok(a),
        _ => Err(format!("\"{}\" isn't an email address I'll send to", a.chars().filter(|c| !c.is_control()).take(80).collect::<String>())),
    }
}

pub fn message_text(from: &str, to: &str, subject: &str, body: &str, now: u64) -> String {
    message_text_in(from, to, subject, body, now, &crate::outbox::Thread::default())
}

/// As `message_text`, as a reply in a conversation: `In-Reply-To` and
/// `References`, so mail programs put it under the message it answers.
pub fn message_text_in(from: &str, to: &str, subject: &str, body: &str, now: u64, thread: &crate::outbox::Thread) -> String {
    // Never a header break, whatever a caller let through.
    let (from, to) = (from.replace(['\r', '\n'], ""), to.replace(['\r', '\n'], ""));
    let (from, to) = (from.as_str(), to.as_str());
    let threading = if thread.in_reply_to.trim().is_empty() {
        String::new()
    } else {
        let refs: Vec<String> = thread.references.iter().map(|r| format!("<{r}>")).collect();
        format!(
            "In-Reply-To: <{}>\r\nReferences: {}\r\n",
            thread.in_reply_to.replace(['\r', '\n'], ""),
            refs.join(" ").replace(['\r', '\n'], "")
        )
    };
    let subject_line = if subject.is_ascii() {
        subject.replace(['\r', '\n'], " ")
    } else {
        format!("=?UTF-8?B?{}?=", crate::b64::encode(subject.replace(['\r', '\n'], " ").as_bytes()))
    };
    let domain = from.rsplit_once('@').map(|(_, d)| d).unwrap_or("atlas.local");
    let id = crate::digest::sha256_hex(format!("{from}{to}{subject}{now}{body}").as_bytes());
    let body = body.replace("\r\n", "\n").replace('\n', "\r\n");
    format!(
        "Date: {}\r\nFrom: {from}\r\nTo: {to}\r\nSubject: {subject_line}\r\nMessage-ID: <{}@{domain}>\r\n{threading}MIME-Version: 1.0\r\n\
         Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: 8bit\r\n\r\n{body}",
        rfc2822_date(now),
        &id[..24]
    )
}

/// `Tue, 30 Sep 2026 18:04:05 +0000`.
pub fn rfc2822_date(t: u64) -> String {
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let days = (t / 86_400) as i64;
    let secs = t % 86_400;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!(
        "{}, {:02} {} {} {:02}:{:02}:{:02} +0000",
        DAYS[(days.rem_euclid(7)) as usize],
        d,
        MONTHS[(m - 1) as usize],
        y,
        secs / 3600,
        (secs / 60) % 60,
        secs % 60
    )
}

/// SMTP's "dot-stuffing": a line that starts with `.` is escaped to `..`
/// so it isn't read as the message terminator. Applied line by line
/// rather than as a single find-replace, since a `.` anywhere but the
/// very start of a line is ordinary content.
fn escape_dot_stuffing(body: &str) -> String {
    body.lines()
        .map(|l| if l.starts_with('.') { format!(".{l}") } else { l.to_string() })
        .collect::<Vec<_>>()
        .join("\r\n")
}

/// A live connection over real TLS: implicit TLS on 465, STARTTLS on 587
/// (Outlook / Microsoft 365, iCloud). The one place this module touches an
/// actual socket -- see `imap::connect` for why this is deliberately the one
/// untested function here; the STARTTLS exchange itself is `Session::starttls`,
/// which is tested. Callers EHLO again after this, as RFC 3207 requires.
pub fn connect(host: &str, port: u16) -> Result<Session<native_tls::TlsStream<std::net::TcpStream>>, String> {
    let tcp = std::net::TcpStream::connect((host, port))
        .map_err(|e| format!("couldn't reach {host}:{port}: {e}"))?;
    // Before TLS, so the handshake is covered too. A server that completes
    // the TCP connection and then goes quiet during the handshake used to
    // hang here with nothing to break it.
    tcp.set_read_timeout(Some(QUIET_FOR))
        .map_err(|e| format!("couldn't set a read timeout on {host}: {e}"))?;
    tcp.set_write_timeout(Some(QUIET_FOR))
        .map_err(|e| format!("couldn't set a write timeout on {host}: {e}"))?;
    let tcp = if port == 587 {
        let mut plain = Session::new(tcp);
        let greeting = plain.read_greeting().map_err(|e| format!("no greeting from {host}: {e}"))?;
        if !greeting.ok() {
            return Err(format!("{host} did not say it was ready: {}", greeting.text()));
        }
        plain.starttls("atlas").map_err(|e| format!("{host}: {e}"))?
    } else {
        tcp
    };
    let connector = native_tls::TlsConnector::new().map_err(|e| format!("couldn't set up TLS: {e}"))?;
    let tls =
        connector.connect(host, tcp).map_err(|e| format!("TLS handshake with {host} failed: {e}"))?;
    let mut session = Session::new(tls);
    if port != 587 {
        let greeting = session.read_greeting().map_err(|e| format!("no greeting from {host}: {e}"))?;
        if !greeting.ok() {
            return Err(format!("{host} did not say it was ready: {}", greeting.text()));
        }
    }
    Ok(session)
}

/// A ceiling on outgoing mail per sending account: 30 an hour, 5 back to
/// back (GCRA, `ratelimit::Gcra`). A drafting loop or a bug that tries to
/// send more is refused with how long to wait, instead of emailing a client
/// thirty times. Chosen, not measured — it is a fuse, not a quota.
pub fn may_send(account: &str, now_ms: u64) -> Result<(), String> {
    static LIMIT: std::sync::OnceLock<std::sync::Mutex<crate::ratelimit::Gcra>> = std::sync::OnceLock::new();
    let lim = LIMIT.get_or_init(|| std::sync::Mutex::new(crate::ratelimit::Gcra::new(30, 3_600_000, 5)));
    match lim.lock().or_else(crate::crash::unpoison) {
        Ok(mut g) => g.check(&account.to_lowercase(), now_ms).map_err(|wait| {
            format!("sending paused: more than 5 in a row from {account} — the next can go in {}s", wait.div_ceil(1000))
        }),
        // Never taken: `unpoison` keeps the limiter (and its counts) after a
        // panic elsewhere, so the limit still holds rather than sends
        // stopping for good (audit Q16).
        Err(_) => Err("sending paused: the send limiter is unavailable".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

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
    fn a_single_line_reply_is_read_correctly() {
        let s = Scripted::new("250 OK\r\n");
        let mut sess = Session::new(s);
        let r = sess.read_reply().unwrap();
        assert_eq!(r.code, 250);
        assert_eq!(r.lines, vec!["OK".to_string()]);
    }

    #[test]
    fn a_multi_line_reply_stops_at_the_line_with_a_space_not_a_dash() {
        let s = Scripted::new("250-example.com at your service\r\n250-STARTTLS\r\n250 AUTH LOGIN PLAIN\r\n");
        let mut sess = Session::new(s);
        let r = sess.read_reply().unwrap();
        assert_eq!(r.code, 250);
        assert_eq!(r.lines.len(), 3);
        assert_eq!(r.lines[2], "AUTH LOGIN PLAIN");
    }

    #[test]
    fn ok_is_true_for_2xx_and_3xx_but_false_for_4xx_and_5xx() {
        assert!(Reply { code: 250, lines: vec![] }.ok());
        assert!(Reply { code: 354, lines: vec![] }.ok());
        assert!(!Reply { code: 450, lines: vec![] }.ok());
        assert!(!Reply { code: 550, lines: vec![] }.ok());
    }

    #[test]
    fn auth_login_sends_username_and_password_base64_encoded_in_order() {
        let script = "334 VXNlcm5hbWU6\r\n334 UGFzc3dvcmQ6\r\n235 Authentication successful\r\n";
        let s = Scripted::new(script);
        let mut sess = Session::new(s);
        sess.auth_login("me@gmail.com", "app-password").unwrap();
        let sent = String::from_utf8_lossy(&sess.stream.sent).into_owned();
        assert!(sent.contains(&crate::b64::encode(b"me@gmail.com")));
        assert!(sent.contains(&crate::b64::encode(b"app-password")));
        // Username before password.
        let user_pos = sent.find(&crate::b64::encode(b"me@gmail.com")).unwrap();
        let pass_pos = sent.find(&crate::b64::encode(b"app-password")).unwrap();
        assert!(user_pos < pass_pos);
    }

    #[test]
    fn auth_login_reports_the_servers_own_reason_on_failure() {
        let script = "334 VXNlcm5hbWU6\r\n334 UGFzc3dvcmQ6\r\n535 Authentication failed: bad app password\r\n";
        let s = Scripted::new(script);
        let mut sess = Session::new(s);
        let e = sess.auth_login("me@gmail.com", "wrong").unwrap_err();
        assert!(e.contains("bad app password"), "got: {e}");
    }

    #[test]
    fn an_address_with_a_second_command_or_recipient_in_it_is_never_sent_to() {
        let s = Scripted::new("250 OK\r\n250 OK\r\n354 go\r\n250 OK\r\n");
        let mut sess = Session::new(s);
        let e = sess.send_mail("me@gmail.com", "a@b.com>\r\nRCPT TO:<evil@x.com", "x", "").unwrap_err();
        assert!(e.contains("isn't an email address"), "{e}");
        assert!(sess.stream.sent.is_empty(), "nothing reached the server");
        for bad in ["Sam <sam@x.com>", "a@b.com,c@d.com", "no-at-sign", "a@localhost", "a@b@c.com", "a b@c.com"] {
            assert!(plain_address(bad).is_err(), "{bad}");
        }
        assert_eq!(plain_address(" sam.o'neil+tag@mail.example.co.uk "), Ok("sam.o'neil+tag@mail.example.co.uk"));
        let m = message_text("me@x.com", "a@b.com\r\nBcc: evil@x.com", "s", "b", 0);
        assert_eq!(m.matches("\r\nBcc:").count(), 0, "{m}");
    }

    #[test]
    fn send_mail_walks_mail_from_rcpt_to_and_data_in_order() {
        let script = "250 OK\r\n250 OK\r\n354 Start mail input\r\n250 OK: queued\r\n";
        let s = Scripted::new(script);
        let mut sess = Session::new(s);
        sess.send_mail("me@gmail.com", "shop@example.com", "unsubscribe", "").unwrap();
        let sent = String::from_utf8_lossy(&sess.stream.sent).into_owned();
        assert!(sent.contains("MAIL FROM:<me@gmail.com>"));
        assert!(sent.contains("RCPT TO:<shop@example.com>"));
        assert!(sent.contains("DATA"));
        assert!(sent.ends_with("\r\n.\r\n"), "the message must end with the SMTP terminator");
    }

    #[test]
    fn a_body_line_starting_with_a_dot_is_escaped_so_it_is_not_read_as_the_terminator() {
        let escaped = escape_dot_stuffing(".this line starts with a dot\nordinary line");
        assert_eq!(escaped, "..this line starts with a dot\r\nordinary line");
    }

    #[test]
    fn mail_from_being_refused_stops_before_rcpt_to_is_ever_sent() {
        let script = "550 Sender address rejected\r\n";
        let s = Scripted::new(script);
        let mut sess = Session::new(s);
        let e = sess.send_mail("blocked@example.com", "shop@example.com", "x", "").unwrap_err();
        assert!(e.contains("MAIL FROM refused"), "got: {e}");
        let sent = String::from_utf8_lossy(&sess.stream.sent).into_owned();
        assert!(!sent.contains("RCPT TO"), "must not proceed to RCPT TO after MAIL FROM was refused");
    }

    #[test]
    fn rcpt_to_being_refused_stops_before_data_is_ever_sent() {
        let script = "250 OK\r\n550 No such recipient\r\n";
        let s = Scripted::new(script);
        let mut sess = Session::new(s);
        let e = sess.send_mail("me@gmail.com", "nobody@example.com", "x", "").unwrap_err();
        assert!(e.contains("RCPT TO refused"), "got: {e}");
        let sent = String::from_utf8_lossy(&sess.stream.sent).into_owned();
        assert!(!sent.contains("\r\nDATA\r\n"), "must not proceed to DATA after RCPT TO was refused");
    }

    #[test]
    fn smtp_auth_xoauth2_succeeds_on_235() {
        let s = Scripted::new("235 2.7.0 Authentication successful\r\n");
        let mut sess = Session::new(s);
        sess.auth_xoauth2("me@outlook.com", "sometoken").unwrap();
    }

    #[test]
    fn smtp_auth_xoauth2_answers_a_334_continuation_before_reading_the_real_failure() {
        let error_json = crate::b64::encode(br#"{"status":"401","schemes":"bearer"}"#);
        let script = format!("334 {error_json}\r\n535 5.7.3 Authentication unsuccessful\r\n");
        let s = Scripted::new(&script);
        let mut sess = Session::new(s);
        let err = sess.auth_xoauth2("me@outlook.com", "badtoken").unwrap_err();
        assert!(err.contains("Authentication unsuccessful"), "got: {err}");
        let sent = String::from_utf8_lossy(&sess.stream.sent).into_owned();
        assert!(sent.ends_with("\r\n\r\n"), "the 334 continuation must be answered with an empty line");
    }

    #[test]
    fn smtp_auth_xoauth2_sends_the_initial_response_with_the_auth_command() {
        let s = Scripted::new("235 2.7.0 Authentication successful\r\n");
        let mut sess = Session::new(s);
        sess.auth_xoauth2("me@outlook.com", "sometoken").unwrap();
        let sent = String::from_utf8_lossy(&sess.stream.sent).into_owned();
        assert!(sent.starts_with("AUTH XOAUTH2 "));
        assert!(sent.contains(&crate::msoauth::xoauth2_string("me@outlook.com", "sometoken")));
    }
    #[test]
    fn starttls_upgrades_only_when_offered_and_nothing_is_smuggled() {
        let s = Session::new(Scripted::new("250-smtp.office365.com\r\n250-SIZE 157286400\r\n250 STARTTLS\r\n220 2.0.0 SMTP server ready\r\n"));
        let stream = s.starttls("atlas").unwrap();
        assert_eq!(String::from_utf8(stream.sent).unwrap(), "EHLO atlas\r\nSTARTTLS\r\n");

        let s = Session::new(Scripted::new("250-mx\r\n250 AUTH LOGIN\r\n"));
        assert!(s.starttls("atlas").err().unwrap_or_default().contains("doesn't offer STARTTLS"));

        let s = Session::new(Scripted::new("250 STARTTLS\r\n454 TLS not available\r\n"));
        assert!(s.starttls("atlas").err().unwrap_or_default().contains("refused STARTTLS"));

        // A reply pipelined behind the 220 would be read as if it came over TLS.
        let s = Session::new(Scripted::new("250 STARTTLS\r\n220 go ahead\r\n250 injected\r\n"));
        assert!(s.starttls("atlas").err().unwrap_or_default().contains("before encryption"));
    }
}

#[cfg(test)]
mod threading {
    #[test]
    fn a_reply_names_the_message_it_answers_and_its_ancestors() {
        let t = crate::outbox::Thread::replying_to("<b@x>", &["a@x".to_string()]);
        let m = super::message_text_in("me@y.com", "sam@x.com", "Re: plans", "Yes.", 1_790_000_000, &t);
        assert!(m.contains("\r\nIn-Reply-To: <b@x>\r\n"), "{m}");
        assert!(m.contains("\r\nReferences: <a@x> <b@x>\r\n"), "{m}");
        let plain = super::message_text("me@y.com", "sam@x.com", "Hello", "Hi.", 1_790_000_000);
        assert!(!plain.contains("In-Reply-To"));
    }

}
