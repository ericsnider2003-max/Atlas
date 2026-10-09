//! **Mail through Himalaya** (`mail.backend: himalaya`, 28 Sep 2026).
//!
//! Atlas's own IMAP returns the body as `BODY[TEXT]` and headers as sent:
//! an encoded subject, a quoted-printable or base64 body, a multipart
//! message's boundaries all arrive undecoded. Himalaya (pimalaya/himalaya,
//! written against its 2.1.0 source) decodes them. These pin how its
//! `--json` output is read, the exact commands it's given (reading leaves a
//! message unread), and a whole mail check through the daemon with a
//! stand-in `himalaya` -- a Python script, skipped where there's none.

use atlas::config::Config;
#[cfg(unix)]
use atlas::daemon::Daemon;
use atlas::himalaya;
#[cfg(unix)]
use atlas::{intent::Intent, mail::Account, platform::{mock::MockPlatform, Monitor}, proactive::{Proactive, ProactiveConfig}, store::Store};
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;

const ENVELOPES: &str = r#"{"envelopes": [
  {"id": "4021", "message-id": "abc@mail.example.com", "in-reply-to": [], "flags": [],
   "subject": "Café on Thursday?", "from": [{"name": "Jane Roe", "email": "jane@client.example"}],
   "to": [{"name": null, "email": "eric@example.com"}], "date": "2026-09-27T09:15:00+01:00", "size": 5120, "has-attachment": null},
  {"id": "4022", "message-id": "def@mail.example.com", "in-reply-to": ["abc@mail.example.com"],
   "flags": [{"raw": "\\Answered", "iana": "answered"}], "subject": "Re: Café on Thursday?",
   "from": [{"name": null, "email": "sam@example.org"}], "to": [], "date": null, "size": 900}
]}"#;

/// `message read --json`: mail-parser's parsed message, as Himalaya prints it.
const MESSAGE: &str = r#"{"html_body": [2], "text_body": [1], "attachments": [], "parts": [
  {"headers": [
      {"name": "subject", "value": {"Text": "Café on Thursday?"}, "offset_field": 0, "offset_start": 0, "offset_end": 0},
      {"name": "cc", "value": {"Address": {"List": [{"name": "Kim", "address": "kim@client.example"}]}}, "offset_field": 0, "offset_start": 0, "offset_end": 0},
      {"name": "references", "value": {"TextList": ["root@mail.example.com", "abc@mail.example.com"]}, "offset_field": 0, "offset_start": 0, "offset_end": 0},
      {"name": "list_unsubscribe", "value": {"Text": "<mailto:unsub@example.com>"}, "offset_field": 0, "offset_start": 0, "offset_end": 0},
      {"name": {"other": "Authentication-Results"}, "value": {"Text": "mx.example.com; spf=pass; dkim=pass"}, "offset_field": 0, "offset_start": 0, "offset_end": 0}
    ], "is_encoding_problem": false, "body": {"Multipart": [1, 2]}, "offset_header": 0, "offset_body": 0, "offset_end": 0},
  {"headers": [], "is_encoding_problem": false, "body": {"Text": "Hi Eric — could we meet at the café at 3? Café crème on me."}, "offset_header": 0, "offset_body": 0, "offset_end": 0},
  {"headers": [], "is_encoding_problem": false, "body": {"Html": "<p>Hi Eric</p>"}, "offset_header": 0, "offset_body": 0, "offset_end": 0}
]}"#;

#[test]
fn envelopes_and_a_decoded_message_become_atlas_messages() {
    let found = himalaya::envelopes(ENVELOPES).unwrap();
    assert_eq!(found.len(), 2);
    let (id, mut m) = found[0].clone();
    assert_eq!(id, "4021");
    assert_eq!(m.uid, 4021);
    assert_eq!(m.from, "Jane Roe <jane@client.example>");
    assert_eq!(m.subject, "Café on Thursday?", "decoded, not =?UTF-8?…?=");
    assert_eq!(m.message_id, "<abc@mail.example.com>");
    assert!(!m.seen && !m.answered);
    assert!(found[1].1.answered);
    assert_eq!(found[1].1.in_reply_to, "<abc@mail.example.com>");

    assert!(himalaya::fill_from_message(&mut m, MESSAGE));
    assert_eq!(m.body, "Hi Eric — could we meet at the café at 3? Café crème on me.", "the plain part, decoded");
    assert_eq!(m.cc, "Kim <kim@client.example>");
    assert_eq!(m.references, "<root@mail.example.com> <abc@mail.example.com>");
    assert_eq!(m.list_unsubscribe, "<mailto:unsub@example.com>");
    assert!(m.authentication_results.contains("dkim=pass"));

    // An HTML-only message is read as text.
    let mut h = atlas::imap::Message::default();
    let html_only = r#"{"html_body": [0], "text_body": [], "parts": [{"headers": [], "is_encoding_problem": false,
        "body": {"Html": "<html><body><p>Your order has <b>shipped</b>.</p><script>x()</script></body></html>"}}]}"#;
    assert!(himalaya::fill_from_message(&mut h, html_only));
    assert!(h.body.contains("Your order has") && h.body.contains("shipped") && !h.body.contains("x()"), "{}", h.body);
    assert!(himalaya::envelopes("not json").is_none());
}

#[test]
fn the_commands_it_is_given_read_without_marking_anything_read() {
    let a = himalaya::search_args("personal", "inbox", 100, &["not", "flag", "seen"]);
    assert_eq!(a, ["--json", "--account", "personal", "envelope", "search", "--mailbox", "inbox", "--page-size", "100", "not", "flag", "seen"]);
    let r = himalaya::read_args("personal", "inbox", "4021");
    assert_eq!(r, ["--json", "--account", "personal", "message", "read", "--mailbox", "inbox", "4021"]);
    assert!(!r.iter().any(|x| x == "--seen"), "since Himalaya 2.0 reading leaves it unread unless told --seen");
    assert_eq!(himalaya::iso_date("01-Sep-2026").as_deref(), Some("2026-09-01"));
    assert_eq!(himalaya::iso_date("28-Sep-2026").as_deref(), Some("2026-09-28"));
    assert_eq!(himalaya::iso_date("nonsense"), None);
    let host = himalaya::as_host("C:\\Tools\\himalaya.exe", "work");
    assert_eq!(himalaya::route(&host), Some(("C:\\Tools\\himalaya.exe".into(), "work".into())));
    assert_eq!(himalaya::route("imap.gmail.com"), None);
}

// ================= a whole check, through the daemon =================

#[cfg(unix)]
fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-himalaya-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// A stand-in `himalaya`: answers `envelope search` and `message read` from
/// the fixtures above and writes every command line it was given to a log.
#[cfg(unix)]
fn stand_in(dir: &Path) -> Option<(PathBuf, PathBuf)> {
    use std::os::unix::fs::PermissionsExt;
    if std::process::Command::new("python3").arg("--version").output().map(|o| !o.status.success()).unwrap_or(true) {
        eprintln!("skipped: no python3 for the stand-in himalaya");
        return None;
    }
    std::fs::write(dir.join("envelopes.json"), ENVELOPES).unwrap();
    std::fs::write(dir.join("message.json"), MESSAGE).unwrap();
    let log = dir.join("calls.log");
    let script = dir.join("himalaya");
    std::fs::write(
        &script,
        format!(
            "#!/usr/bin/env python3\nimport sys, os\nd = {dir:?}\nopen(os.path.join(d, 'calls.log'), 'a').write(' '.join(sys.argv[1:]) + '\\n')\n\
             a = sys.argv[1:]\n\
             if 'search' in a and 'sent' in a:\n    print('{{\"envelopes\": []}}')\n\
             elif 'search' in a:\n    print(open(os.path.join(d, 'envelopes.json')).read())\n\
             elif 'read' in a:\n    print(open(os.path.join(d, 'message.json')).read())\n\
             else:\n    sys.stderr.write('unknown command\\n'); sys.exit(2)\n",
            dir = dir.display().to_string()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    Some((script, log))
}

#[cfg(unix)]
#[test]
fn a_mail_check_goes_through_himalaya_with_nothing_from_the_vault() {
    let dir = tmp("check");
    let Some((script, log)) = stand_in(&dir) else { return };
    let mut c = Config::load(Path::new("config")).unwrap();
    {
        let t = c.tools.as_mut().unwrap();
        t.mail.enabled = true;
        t.mail.backend = "himalaya".into();
        t.mail.himalaya = script.display().to_string();
        // No vault entry, no server: Himalaya has its own.
        t.mail.accounts = vec![Account { name: "personal".into(), address: "eric@example.com".into(), ..Default::default() }];
    }
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("check-store")), Proactive::new(ProactiveConfig::default()));
    d.connectivity.set(atlas::connectivity::Reach::Online, 0);
    let said = d.execute(&Intent::Mail(String::new()));
    assert!(!said.contains("couldn't get at any"), "the vault wasn't needed: {said}");
    let after = d.errands_done_for_test().join(" ");
    let calls = std::fs::read_to_string(&log).unwrap();
    assert!(calls.contains("--json --account personal envelope search --mailbox inbox --page-size 100 not flag seen"), "{calls}");
    assert!(calls.contains("message read --mailbox inbox 4021") && calls.contains("message read --mailbox inbox 4022"), "{calls}");
    assert!(!calls.contains("--seen"), "{calls}");
    assert!(!after.to_lowercase().contains("couldn't reach") && !after.contains("Himalaya said"), "{after}");
    assert!(after.contains('2') || after.to_lowercase().contains("two") || after.contains("Jane") || after.contains("Café"), "the two messages were read: {after}");
}

#[test]
fn the_shipped_setting_is_atlas_own_imap() {
    let c = Config::load(Path::new("config")).unwrap();
    let m = c.tools.unwrap().mail;
    assert_eq!(m.backend, "imap");
    assert!(!m.by_himalaya());
    assert_eq!(m.himalaya, "himalaya");
}
