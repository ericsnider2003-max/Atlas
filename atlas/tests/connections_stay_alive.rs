//! A connection is kept alive, not just made (N5, the connections design's
//! lifecycle, 6b): one refresh at a time, a token kept while it has time
//! left, a refused sign-in noticed once and not asked again, and taking a
//! connection away taken back at the provider where it can be.

use atlas::connect::{self, CalendarLink};
use atlas::oauthlink::{self, Provider};
use atlas::social::apis::{self, GoogleSignIn, Net, Reply};
use atlas::store::Store;
use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn reply(status: u16, body: &str) -> Reply {
    Reply { status, body: body.into(), last_modified: None, retry_after: None }
}

/// Answers every token request with the same body, and keeps what was asked.
struct Says {
    body: String,
    status: u16,
    asked: RefCell<Vec<String>>,
}

impl Says {
    fn new(status: u16, body: &str) -> Says {
        Says { body: body.into(), status, asked: RefCell::new(vec![]) }
    }
}

impl Net for Says {
    fn get(&self, host: &str, path: &str, _: &[(&str, &str)]) -> Result<Reply, String> {
        self.asked.borrow_mut().push(format!("GET {host}{path}"));
        Ok(reply(200, r#"{"items":[]}"#))
    }
    fn post_form(&self, host: &str, path: &str, form: &str) -> Result<Reply, String> {
        self.asked.borrow_mut().push(format!("POST {host}{path} {form}"));
        Ok(reply(self.status, &self.body))
    }
    fn post_json(&self, _: &str, _: &str, _: &[(&str, &str)], _: &str) -> Result<Reply, String> {
        Err("not used".into())
    }
}

#[test]
fn eight_threads_wanting_one_sign_in_make_one_request() {
    let fetched = Arc::new(AtomicUsize::new(0));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let fetched = fetched.clone();
            std::thread::spawn(move || {
                connect::access_once("herd test-token-1", 1_000, || {
                    fetched.fetch_add(1, Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    Ok(("AT-1".into(), 3600))
                })
            })
        })
        .collect();
    for t in threads {
        assert_eq!(t.join().unwrap().unwrap(), "AT-1");
    }
    assert_eq!(fetched.load(Ordering::SeqCst), 1, "one refresh, not eight");
}

#[test]
fn a_token_is_kept_while_it_has_time_left_and_renewed_before_it_runs_out() {
    let n = AtomicUsize::new(0);
    let fetch = || {
        let i = n.fetch_add(1, Ordering::SeqCst);
        Ok((format!("AT-{i}"), 3600))
    };
    assert_eq!(connect::access_once("keep test-token-2", 10_000, fetch).unwrap(), "AT-0");
    // Half an hour on: still good for another half hour.
    assert_eq!(connect::access_once("keep test-token-2", 11_800, fetch).unwrap(), "AT-0");
    // Four minutes before it runs out: inside the margin, fetched again.
    let edge = 10_000 + 3600 - 240;
    assert_eq!(connect::access_once("keep test-token-2", edge, fetch).unwrap(), "AT-1");
    assert_eq!(n.load(Ordering::SeqCst), 2);
    // Disconnected: forgotten, so the next ask fetches.
    connect::forget_access("keep test-token-2");
    assert_eq!(connect::access_once("keep test-token-2", edge, fetch).unwrap(), "AT-2");
}

#[test]
fn a_failure_is_never_kept() {
    let n = AtomicUsize::new(0);
    let failing = || {
        n.fetch_add(1, Ordering::SeqCst);
        Err::<(String, u64), String>("offline".into())
    };
    assert!(connect::access_once("fail test-token-3", 1, failing).is_err());
    assert!(connect::access_once("fail test-token-3", 2, failing).is_err());
    assert_eq!(n.load(Ordering::SeqCst), 2, "each ask tries again");
    assert_eq!(connect::access_once("fail test-token-3", 3, || Ok(("AT".into(), 3600))).unwrap(), "AT");
}

#[test]
fn a_signed_in_calendar_reads_with_one_token_an_hour_not_one_a_read() {
    let net = Says::new(200, r#"{"access_token":"AT","expires_in":3600}"#);
    oauthlink::calendar_ics(&net, Provider::Microsoft, "RT-alive-1", 1_790_000_000).unwrap();
    oauthlink::calendar_ics(&net, Provider::Microsoft, "RT-alive-1", 1_790_000_900).unwrap();
    let posts = net.asked.borrow().iter().filter(|a| a.starts_with("POST")).count();
    assert_eq!(posts, 1, "the second read used the token still good: {:?}", net.asked.borrow());
}

#[test]
fn every_refused_sign_in_is_told_from_a_passing_failure() {
    let refused = Says::new(400, r#"{"error":"invalid_grant","error_description":"AADSTS70008: expired"}"#);
    let e = oauthlink::calendar_ics(&refused, Provider::Microsoft, "RT-refused-1", 1_790_000_000).unwrap_err();
    assert!(connect::revoked(&e), "{e}");

    let g = GoogleSignIn { client_id: "c".into(), client_secret: "s".into(), refresh_token: "RT-refused-2".into(), obtained: 0 };
    let refused = Says::new(400, r#"{"error":"invalid_grant"}"#);
    let e = apis::google_access(&refused, &g).unwrap_err();
    assert!(connect::revoked(&e), "{e}");

    // Something a later try could get past is not a refusal.
    let busy = Says::new(503, r#"{"error":"temporarily_unavailable"}"#);
    let e = oauthlink::calendar_ics(&busy, Provider::Microsoft, "RT-busy-1", 1_790_000_000).unwrap_err();
    assert!(!connect::revoked(&e), "{e}");
    assert!(!connect::revoked("couldn't reach login.microsoftonline.com"));
}

#[test]
fn a_refused_calendar_is_said_once_and_not_asked_again_until_signed_in_again() {
    let mut l = CalendarLink { name: "Google Calendar (me@gmail.com)".into(), url: oauthlink::calendar_key(Provider::Google, "me@gmail.com"), last_read: 0, ..Default::default() };
    assert!(connect::read_due(&l, 10_000));
    let refused = format!("Google {} (it was revoked or expired) -- sign in again", connect::REVOKED);

    let said = connect::took_read(&mut l, Some(&refused)).expect("said when it stopped");
    assert!(said.contains("Google Calendar (me@gmail.com)") && said.contains("Sign in again"), "{said}");
    assert!(l.needs_signin && !l.last_ok);
    assert!(!connect::read_due(&l, 10_000_000), "a refused sign-in isn't asked again every 15 minutes");
    assert_eq!(connect::took_read(&mut l, Some(&refused)), None, "said once, not on every read");

    // A network hiccup is not a refusal, and says nothing.
    let mut m = CalendarLink { name: "Outlook".into(), ..Default::default() };
    assert_eq!(connect::took_read(&mut m, Some("couldn't reach graph.microsoft.com")), None);
    assert!(!m.needs_signin && connect::read_due(&m, 10_000));

    // Read fine again (signed in again): back to normal.
    connect::took_read(&mut l, None);
    assert!(!l.needs_signin && l.last_ok);
}

#[test]
fn a_mailbox_whose_sign_in_was_refused_is_not_tried_until_it_works_again() {
    let dir = std::env::temp_dir().join("atlas-stay-alive-mail");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::new(dir.clone());
    assert!(!connect::sign_in_refused(&store, "me@outlook.com"), "never tried");
    connect::note_health(&store, "me@outlook.com", Some("couldn't reach outlook.office365.com"));
    assert!(!connect::sign_in_refused(&store, "me@outlook.com"), "a passing failure is tried again");
    connect::note_health(&store, "Me@Outlook.com", Some("Microsoft no longer accepts Atlas's sign-in for this account (revoked or expired) -- sign in again"));
    assert!(connect::sign_in_refused(&store, "me@outlook.com"));
    connect::note_health(&store, "me@outlook.com", None);
    assert!(!connect::sign_in_refused(&store, "me@outlook.com"), "signed in again");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn taking_a_google_sign_in_back_asks_google_to_end_it() {
    let net = Says::new(200, "");
    oauthlink::revoke_google(&net, "1//RT+x").unwrap();
    assert_eq!(net.asked.borrow()[0], "POST oauth2.googleapis.com/revoke token=1%2F%2FRT%2Bx");
    // Already gone at Google is what was wanted.
    assert!(oauthlink::revoke_google(&Says::new(400, r#"{"error":"invalid_token"}"#), "x").is_ok());
    assert!(oauthlink::revoke_google(&Says::new(500, "oops"), "x").is_err());
}

#[test]
fn every_way_a_sign_in_ends_is_wired() {
    let connecting = crate::common::source_of("connecting");
    // Calendar and Outlook rows get one fix button when refused.
    assert!(connecting.contains("Sign in again") && connecting.contains("sign_in_refused"));
    // Disconnecting a Google calendar ends the grant at Google; Microsoft's own page is named.
    assert!(connecting.contains("revoke_google") && connecting.contains("MICROSOFT_PERMISSIONS"));
    // The scheduled mail check skips a refused mailbox.
    let inbox = crate::common::source_of("daemon/inbox");
    assert!(inbox.contains("sign_in_refused(&store, &account.address)"));
    // Mail asks through the kept token, not a refresh per connection.
    let daemon = crate::common::source_of("daemon");
    assert!(daemon.contains("msoauth::access(client_id, password)") && !daemon.contains("msoauth::refresh(client_id, password)"));
    // YouTube can be taken away, and its grant ended at Google.
    let glue = crate::common::source_of("social/glue");
    assert!(glue.contains("\"youtube-disconnect\" => self.youtube_disconnect(t)") && glue.contains("revoke_google"));
}

#[test]
fn a_connected_youtube_row_has_a_disconnect_button() {
    let v = atlas::social::page::View {
        services: vec![atlas::social::page::Service {
            name: "YouTube".into(),
            state: "Connected".into(),
            connected: true,
            button: Some(("google".into(), "Connect again".into())),
            inner: String::new(),
            note: String::new(),
            disconnect: Some("youtube-disconnect".into()),
        }],
        ..Default::default()
    };
    let html = atlas::social::page::render_social(&v);
    assert!(html.contains("value='youtube-disconnect'><button>Disconnect</button>"), "{html}");
}

#[test]
fn taking_a_tiktok_sign_in_back_asks_tiktok_to_end_it() {
    let s = apis::TikTokSignIn {
        client_key: "ck".into(),
        client_secret: "c s".into(),
        redirect: "https://x.example/r".into(),
        refresh_token: "rt".into(),
        state: String::new(),
        obtained: 0,
    };
    let net = Says::new(200, r#"{"error":{"code":"ok"}}"#);
    apis::tiktok_revoke(&net, &s, "act.1").unwrap();
    assert_eq!(net.asked.borrow()[0], "POST open.tiktokapis.com/v2/oauth/revoke/ client_key=ck&client_secret=c%20s&token=act.1");
    // Already gone at TikTok is what was wanted; a refusal is said.
    assert!(apis::tiktok_revoke(&Says::new(400, r#"{"error":"access_token_invalid"}"#), &s, "x").is_ok());
    assert!(apis::tiktok_revoke(&Says::new(500, "oops"), &s, "x").is_err());
}
