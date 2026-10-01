//! Reading mail through Himalaya, when you choose it (`mail.backend:
//! himalaya`).
//!
//! **Source:** `pimalaya/himalaya` (MIT or Apache-2.0), a command-line mail
//! client, driven as a program with its `--json` output -- written against
//! its 2.1.0 source (28 Sep 2026): `envelope search [QUERY]` with its query
//! language (`not flag seen`, `after <yyyy-mm-dd>`), `-m/--mailbox` (an
//! alias such as `inbox` or `sent` from its own config), and `message read
//! <ID>`, which since 2.0 leaves a message unread unless told `--seen`. Its
//! `--json` message is `mail-parser`'s parsed message: every part decoded.
//!
//! **Why it's offered.** Atlas's own IMAP (`imap`) is solid at the protocol
//! -- literals, timeouts, XOAUTH2 -- but it hands the body back as the raw
//! `BODY[TEXT]` and headers as sent: a `=?UTF-8?B?…?=` subject stays
//! encoded, a quoted-printable or base64 body stays encoded, a multipart
//! message arrives with its boundaries. Himalaya's parser decodes all of
//! that, and speaks JMAP, Gmail's and Microsoft's own APIs and local
//! Maildir as well as IMAP. It keeps its own accounts and passwords
//! (`himalaya configure`), so nothing from Atlas's vault is handed to it.
//!
//! Default stays `imap`: Himalaya is a separate install, with its own setup.

use crate::imap::Message;
use serde_json::Value;

/// The arguments for a search: JSON out, the account, the mailbox, a page
/// of at most `most`, and the query.
pub fn search_args(account: &str, mailbox: &str, most: u32, query: &[&str]) -> Vec<String> {
    let mut a: Vec<String> = vec!["--json".into()];
    if !account.trim().is_empty() {
        a.push("--account".into());
        a.push(account.trim().into());
    }
    a.extend(["envelope".into(), "search".into(), "--mailbox".into(), mailbox.into(), "--page-size".into(), most.to_string()]);
    a.extend(query.iter().map(|q| q.to_string()));
    a
}

/// The arguments to read one message, leaving it unread.
pub fn read_args(account: &str, mailbox: &str, id: &str) -> Vec<String> {
    let mut a: Vec<String> = vec!["--json".into()];
    if !account.trim().is_empty() {
        a.push("--account".into());
        a.push(account.trim().into());
    }
    a.extend(["message".into(), "read".into(), "--mailbox".into(), mailbox.into(), id.into()]);
    a
}

/// `01-Sep-2026` (IMAP's form, `triage::imap_date`) as `2026-09-01`, the
/// form Himalaya's `after` takes.
pub fn iso_date(imap_date: &str) -> Option<String> {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let mut it = imap_date.trim().split('-');
    let d: u32 = it.next()?.parse().ok()?;
    let month = it.next()?;
    let m = MONTHS.iter().position(|x| x.eq_ignore_ascii_case(month))? + 1;
    let y: u32 = it.next()?.parse().ok()?;
    Some(format!("{y:04}-{m:02}-{d:02}"))
}

fn addresses(v: &Value) -> String {
    let one = |a: &Value| -> Option<String> {
        let email = a["email"].as_str().or(a["address"].as_str())?.to_string();
        Some(match a["name"].as_str().filter(|n| !n.trim().is_empty()) {
            Some(n) => format!("{n} <{email}>"),
            None => email,
        })
    };
    match v {
        Value::Array(list) => list.iter().filter_map(one).collect::<Vec<_>>().join(", "),
        Value::Object(_) => one(v).unwrap_or_default(),
        _ => String::new(),
    }
}

/// One envelope as a message with no body yet.
fn envelope(e: &Value) -> Option<(String, Message)> {
    let id = match &e["id"] {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    let flags: Vec<String> = e["flags"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| f["iana"].as_str().or(f["raw"].as_str()).or(f.as_str()).map(|s| s.trim_start_matches(['\\', '$']).to_lowercase()))
        .collect();
    let bracket = |s: &str| if s.is_empty() || s.starts_with('<') { s.to_string() } else { format!("<{s}>") };
    let m = Message {
        uid: id.parse().unwrap_or(0),
        from: addresses(&e["from"]),
        to: addresses(&e["to"]),
        subject: e["subject"].as_str().unwrap_or_default().to_string(),
        date: e["date"].as_str().unwrap_or_default().to_string(),
        message_id: bracket(e["message-id"].as_str().unwrap_or_default()),
        in_reply_to: e["in-reply-to"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|s| s.as_str())
            .map(bracket)
            .unwrap_or_default(),
        seen: flags.iter().any(|f| f == "seen"),
        answered: flags.iter().any(|f| f == "answered"),
        ..Default::default()
    };
    Some((id, m))
}

/// The envelopes out of `envelope search --json`: `{"envelopes": [...]}`,
/// or a bare list. `None` when it isn't that at all.
pub fn envelopes(json: &str) -> Option<Vec<(String, Message)>> {
    let v: Value = serde_json::from_str(json).ok()?;
    let list = match &v {
        Value::Array(a) => a,
        Value::Object(o) => o.get("envelopes")?.as_array()?,
        _ => return None,
    };
    Some(list.iter().filter_map(envelope).collect())
}

fn header_text(v: &Value) -> String {
    match v {
        Value::Object(o) => {
            if let Some(t) = o.get("Text").and_then(|t| t.as_str()) {
                return t.to_string();
            }
            if let Some(l) = o.get("TextList").and_then(|l| l.as_array()) {
                return l.iter().filter_map(|s| s.as_str()).map(|s| format!("<{}>", s.trim_matches(['<', '>']))).collect::<Vec<_>>().join(" ");
            }
            String::new()
        }
        Value::String(s) => s.clone(),
        _ => String::new(),
    }
}

/// What `message read --json` adds to an envelope: the decoded text body
/// (the plain parts, or the HTML one made text when there are none), and
/// the headers Atlas reads that an envelope doesn't carry.
pub fn fill_from_message(m: &mut Message, json: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(json) else { return false };
    let parts = v["parts"].as_array().cloned().unwrap_or_default();
    let part_text = |i: &Value, key: &str| -> Option<String> {
        let p = parts.get(i.as_u64()? as usize)?;
        p["body"][key].as_str().map(str::to_string)
    };
    let mut body: Vec<String> = v["text_body"].as_array().into_iter().flatten().filter_map(|i| part_text(i, "Text")).collect();
    if body.is_empty() {
        body = v["html_body"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|i| part_text(i, "Html").or_else(|| part_text(i, "Text")))
            .map(|h| crate::research::strip_html(&h))
            .collect();
    }
    m.body = body.join("\n\n").trim().to_string();
    if let Some(headers) = parts.first().and_then(|p| p["headers"].as_array()) {
        for h in headers {
            let name = match &h["name"] {
                Value::String(s) => s.to_lowercase(),
                Value::Object(o) => o.get("other").or_else(|| o.get("Other")).and_then(|s| s.as_str()).unwrap_or_default().to_lowercase(),
                _ => continue,
            };
            let value = header_text(&h["value"]);
            match name.as_str() {
                "references" if m.references.is_empty() => m.references = value,
                "list_unsubscribe" | "list-unsubscribe" if m.list_unsubscribe.is_empty() => m.list_unsubscribe = value,
                "authentication-results" if m.authentication_results.is_empty() => m.authentication_results = value,
                "cc" if m.cc.is_empty() => m.cc = addresses(&h["value"]["Address"]["List"]),
                _ => {}
            }
        }
    }
    true
}

/// Run Himalaya, with no window. Its own error, said plainly, when it
/// fails.
fn run(program: &str, args: &[String]) -> Result<String, String> {
    let out = crate::tools::command(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("couldn't start Himalaya ({program}): {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let line = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("it stopped with an error").trim().to_string();
        return Err(format!("Himalaya said: {line}"));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Messages in one mailbox matching a query, each read (and left unread).
fn fetch(program: &str, account: &str, mailbox: &str, query: &[&str], most: u32) -> Result<Vec<Message>, String> {
    let listed = run(program, &search_args(account, mailbox, most, query))?;
    let found = envelopes(&listed).ok_or_else(|| "Himalaya's list wasn't the JSON it should be".to_string())?;
    let mut out = Vec::with_capacity(found.len());
    for (id, mut m) in found {
        // A message that won't read is kept with its envelope: the sender
        // and subject still count.
        if let Ok(json) = run(program, &read_args(account, mailbox, &id)) {
            fill_from_message(&mut m, &json);
        }
        out.push(m);
    }
    Ok(out)
}

/// The inbox's unread mail, and what you sent since `sent_since` (IMAP's
/// date form) -- the same pair `connect_and_fetch_inbox` gives back. The
/// mailboxes are Himalaya's `inbox` and `sent` aliases.
pub fn fetch_inbox(program: &str, account: &str, sent_since: Option<&str>) -> Result<(Vec<Message>, Result<Vec<Message>, String>), String> {
    let unread = fetch(program, account, "inbox", &["not", "flag", "seen"], 100)?;
    let sent = match sent_since.and_then(iso_date) {
        Some(day) => fetch(program, account, "sent", &["after", &day], 300),
        None => Ok(Vec::new()),
    };
    Ok((unread, sent))
}

/// Everything in the inbox since a day (IMAP's date form), read or not.
pub fn fetch_since(program: &str, account: &str, since: &str) -> Result<Vec<Message>, String> {
    let day = iso_date(since).ok_or_else(|| format!("{since} isn't a date I can hand to Himalaya"))?;
    fetch(program, account, "inbox", &["after", &day], 500)
}

/// The arguments to send a whole message read from standard input
/// (`message send`, which also saves a copy to the Sent folder).
pub fn send_args(account: &str) -> Vec<String> {
    let mut a: Vec<String> = Vec::new();
    if !account.trim().is_empty() {
        a.push("--account".into());
        a.push(account.trim().into());
    }
    a.extend(["message".into(), "send".into()]);
    a
}

/// Send one message through Himalaya, with its own account and password
/// (30 Sep 2026 sweep: with `backend: himalaya` mail was read through it but
/// sent over Atlas's own SMTP with a vault password that isn't there, so
/// every send failed).
pub(crate) fn send(program: &str, account: &str, message: &str) -> Result<(), String> {
    use std::io::Write;
    let mut child = crate::tools::command(program)
        .args(send_args(account))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("couldn't start Himalaya ({program}): {e}"))?;
    if let Some(mut input) = child.stdin.take() {
        input.write_all(message.as_bytes()).map_err(|e| format!("couldn't hand Himalaya the message: {e}"))?;
    }
    let out = child.wait_with_output().map_err(|e| format!("lost track of Himalaya: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let line = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("it stopped with an error").trim().to_string();
        return Err(format!("Himalaya said: {line}"));
    }
    Ok(())
}

/// The account's mail handed to the crew's mail code with Himalaya as its
/// "server": the fetch functions that take a host (`connect_and_fetch_inbox`
/// and `connect_and_fetch_since`) see this and ask Himalaya instead. Nothing
/// else in those errands changes.
pub fn as_host(program: &str, account: &str) -> String {
    format!("{HOST_MARK}{program}\u{1f}{account}")
}

const HOST_MARK: &str = "himalaya:";

/// The program and account, when a host is Himalaya's.
pub fn route(host: &str) -> Option<(String, String)> {
    let rest = host.strip_prefix(HOST_MARK)?;
    let (program, account) = rest.split_once('\u{1f}')?;
    Some((program.to_string(), account.to_string()))
}
