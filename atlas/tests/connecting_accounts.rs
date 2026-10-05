//! Connecting an account from what you type (2 Oct 2026, Eric: "the
//! connection process doesn't seem like it was actually thought out").

use atlas::config::Config;
use atlas::connect::{self, Way};
use atlas::daemon::Daemon;
use atlas::hub::Page;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::server::Action;
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-connecting-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn fields(kv: &[(&str, &str)]) -> Vec<(String, String)> {
    kv.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

#[test]
fn the_address_says_what_connecting_it_takes() {
    match connect::way_for(" Eric@Gmail.com ") {
        Some(Way::AppPassword(m)) => {
            assert_eq!((m.provider.as_str(), m.imap_host.as_str(), m.imap_port), ("Gmail", "imap.gmail.com", 993));
            assert_eq!(m.link.as_deref(), Some("https://myaccount.google.com/apppasswords"));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(connect::way_for("me@hotmail.com"), Some(Way::MicrosoftSignIn));
    assert!(matches!(connect::way_for("me@icloud.com"), Some(Way::AppPassword(m)) if m.imap_host == "imap.mail.me.com"));
    assert!(matches!(connect::way_for("me@proton.me"), Some(Way::NotPossible { .. })));
    assert_eq!(connect::way_for("me@smallbakery.co"), Some(Way::LookItUp { domain: "smallbakery.co".into() }));
    for not in ["eric", "eric@", "@gmail.com", "eric@gmail", "a b@gmail.com", "a@@b.com"] {
        assert_eq!(connect::way_for(not), None, "{not}");
    }
    assert_eq!(connect::ispdb_url("smallbakery.co"), "https://autoconfig.thunderbird.net/v1.1/smallbakery.co");
}

#[test]
fn an_unknown_provider_is_read_from_its_autoconfig() {
    let xml = r#"<?xml version="1.0"?><clientConfig version="1.1"><emailProvider id="example.org">
        <incomingServer type="pop3"><hostname>pop.example.org</hostname><port>995</port><socketType>SSL</socketType></incomingServer>
        <incomingServer type="imap"><hostname>plain.example.org</hostname><port>143</port><socketType>STARTTLS</socketType></incomingServer>
        <incomingServer type="imap"><hostname>imap.example.org</hostname><port>993</port><socketType>SSL</socketType></incomingServer>
        </emailProvider></clientConfig>"#;
    assert_eq!(connect::imap_from_autoconfig(xml), Some(("imap.example.org".to_string(), 993)));
    assert_eq!(connect::imap_from_autoconfig("<clientConfig/>"), None);
    // A template hostname (`%EMAILDOMAIN%`) isn't an address.
    let templ = r#"<incomingServer type="imap"><hostname>imap.%EMAILDOMAIN%</hostname><port>993</port><socketType>SSL</socketType></incomingServer>"#;
    assert_eq!(connect::imap_from_autoconfig(templ), None);
}

#[test]
fn a_calendar_link_is_https_only() {
    assert_eq!(connect::calendar_link("webcal://p01-caldav.icloud.com/published/2/abc").unwrap(), "https://p01-caldav.icloud.com/published/2/abc");
    assert!(connect::calendar_link("http://calendar.google.com/x.ics").is_err(), "a private link isn't sent over plain http");
    assert!(connect::calendar_link("my calendar").is_err());
    assert_eq!(connect::calendar_name("https://calendar.google.com/calendar/ical/x/private-y/basic.ics"), "Google Calendar");
    assert_eq!(connect::calendar_name("https://outlook.office365.com/owa/calendar/x/reachcalendar.ics"), "Outlook calendar");
    let fresh = connect::CalendarLink { last_read: 1_000, ..Default::default() };
    assert!(!connect::read_due(&fresh, 1_000 + connect::READ_EVERY_SECS - 1));
    assert!(connect::read_due(&fresh, 1_000 + connect::READ_EVERY_SECS));
}

#[test]
fn how_an_account_last_went_is_kept() {
    let store = Store::new(tmp("health"));
    assert_eq!(connect::health_of(&store, "a@b.com"), None);
    connect::note_health(&store, "A@B.com", Some("AUTHENTICATIONFAILED"));
    let h = connect::health_of(&store, "a@b.com").unwrap();
    assert_eq!((h.ok, h.said.as_str()), (false, "AUTHENTICATIONFAILED"));
    connect::note_health(&store, "a@b.com", None);
    assert!(connect::health_of(&store, "a@b.com").unwrap().ok);
}

#[test]
fn the_accounts_page_leads_with_connecting_and_walks_you_through_it() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let store = tmp("page");
    let mut d = Daemon::new(&c, &p, None, Store::new(store.clone()), Proactive::new(ProactiveConfig::default()));
    let page = atlas::hublive::reply(&mut d, Action::Hub(Page::Accounts)).body;
    assert!(page.contains("<h2 id=connect-h>Connect an account</h2>"), "{page}");
    assert!(page.contains("Nothing connected yet."));

    // Next, with a Gmail address: back to the page, with the step for it.
    let r = atlas::hublive::reply(&mut d, Action::HubPost { path: "/hub/connect".into(), fields: fields(&[("what", "start"), ("address", "eric@gmail.com")]) });
    assert_eq!(r.status, 303);
    assert!(r.body.starts_with("/hub/accounts?connect=eric%40gmail.com"), "{}", r.body);
    let step = atlas::hublive::reply(&mut d, Action::HubQ(Page::Accounts, "connect=eric%40gmail.com".into())).body;
    assert!(step.contains("Open Gmail's app password page"), "{step}");
    assert!(step.contains("name=host value='imap.gmail.com'"));
    // The vault is shut and has no passphrase yet: the form asks for one.
    assert!(step.contains("Choose a vault passphrase"));

    // A password with the vault shut and no passphrase: nothing is tried or kept.
    let r = atlas::hublive::reply(&mut d, Action::HubPost {
        path: "/hub/connect".into(),
        fields: fields(&[("what", "password"), ("address", "eric@gmail.com"), ("host", "imap.gmail.com"), ("port", "993"), ("password", "abcd efgh")]),
    });
    assert!(r.body.contains("Nothing+was+connected"), "{}", r.body);
    let kept: Vec<atlas::mail::Account> = Store::new(store.clone()).load(atlas::daemon::CONNECTED_ACCOUNTS);
    assert_eq!(kept.len(), 0);

    // A calendar by its link: kept, listed, and taken off again.
    let r = atlas::hublive::reply(&mut d, Action::HubPost { path: "/hub/connect".into(), fields: fields(&[("what", "calendar"), ("url", "webcal://calendar.google.com/calendar/ical/me/private-x/basic.ics")]) });
    assert!(r.body.contains("Added+Google+Calendar"), "{}", r.body);
    let page = atlas::hublive::reply(&mut d, Action::Hub(Page::Accounts)).body;
    assert!(page.contains("<b>Google Calendar</b> calendar <span class=tag>Not tried yet</span>"), "{page}");
    let links: Vec<connect::CalendarLink> = Store::new(store.clone()).load(connect::CALENDAR_LINKS);
    assert_eq!(links[0].url, "https://calendar.google.com/calendar/ical/me/private-x/basic.ics");
    atlas::hublive::reply(&mut d, Action::HubPost { path: "/hub/connect".into(), fields: fields(&[("what", "disconnect"), ("kind", "calendar"), ("id", &links[0].url)]) });
    let links: Vec<connect::CalendarLink> = Store::new(store).load(connect::CALENDAR_LINKS);
    assert_eq!(links.len(), 0);

    // The background reading: with nothing linked, a tick changes nothing.
    atlas::connecting::tick(&mut d, 1_790_740_000);
    let links: Vec<connect::CalendarLink> = Store::new(tmp("page-after")).load(connect::CALENDAR_LINKS);
    assert_eq!(links.len(), 0);

    // Not an address: said, not guessed at.
    let r = atlas::hublive::reply(&mut d, Action::HubPost { path: "/hub/connect".into(), fields: fields(&[("what", "start"), ("address", "eric")]) });
    assert!(r.body.contains("look+like+an+email+address"), "{}", r.body);
}

// ------------------------------------------------------------ one click (4 Oct 2026)
//
// Eric registered Atlas with Google and Microsoft so connecting is one click:
// "as easy as possible but still keep it secure".

use atlas::oauthlink::{self, Provider};
use atlas::social::apis::{Net, Reply};
use std::cell::RefCell;

fn b64url(s: &str) -> String {
    atlas::b64::encode(s.as_bytes()).replace('+', "-").replace('/', "_").trim_end_matches('=').to_string()
}

fn id_token(claims: &str) -> String {
    format!("{}.{}.sig", b64url(r#"{"alg":"RS256"}"#), b64url(claims))
}

#[test]
fn the_consent_page_carries_atlas_s_registration_and_pkce() {
    let g = oauthlink::consent_url(Provider::Google, "http://127.0.0.1:5555", "st8", "chal");
    assert!(g.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"), "{g}");
    assert!(g.contains(&format!("client_id={}", atlas::research::urlencode(oauthlink::GOOGLE_CLIENT_ID))));
    assert!(g.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A5555"));
    assert!(g.contains("calendar.readonly") && !g.contains("gmail"), "read-only calendar, never Gmail: {g}");
    assert!(g.contains("code_challenge=chal&code_challenge_method=S256") && g.contains("state=st8") && g.contains("access_type=offline"));

    let m = oauthlink::consent_url(Provider::Microsoft, "http://localhost:5555", "st8", "chal");
    assert!(m.starts_with("https://login.microsoftonline.com/common/oauth2/v2.0/authorize?"), "{m}");
    assert!(m.contains("client_id=0c54be08-ddff-4d25-ab6f-9791070e3b2a"));
    assert!(m.contains("redirect_uri=http%3A%2F%2Flocalhost%3A5555"));
    for scope in ["IMAP.AccessAsUser.All", "SMTP.Send", "Calendars.Read", "offline_access"] {
        assert!(m.contains(scope), "{scope}: {m}");
    }
    assert!(m.contains("code_challenge_method=S256"));
    assert_eq!(Provider::Google.redirect(80), "http://127.0.0.1:80");
    assert_eq!(Provider::Microsoft.redirect(80), "http://localhost:80");
    // RFC 7636's own example.
    assert_eq!(oauthlink::challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
}

#[test]
fn only_the_answer_to_this_sign_in_is_taken() {
    let p = Provider::Microsoft;
    assert_eq!(oauthlink::code_from(p, "GET /?code=abc%2Fd&state=s1 HTTP/1.1", "s1"), Ok("abc/d".into()));
    assert!(oauthlink::code_from(p, "GET /?code=abc&state=other HTTP/1.1", "s1").unwrap_err().contains("didn't match"));
    assert!(oauthlink::code_from(p, "GET /?error=access_denied&state=s1 HTTP/1.1", "s1").unwrap_err().contains("didn't allow it"));
    let e = oauthlink::code_from(p, "GET /?error=invalid_request&error_description=AADSTS50011+redirect+mismatch&state=s1 HTTP/1.1", "s1").unwrap_err();
    assert!(e.contains("Microsoft said no") && e.contains("AADSTS50011 redirect mismatch"), "{e}");
    assert!(oauthlink::code_from(p, "GET /?state=s1 HTTP/1.1", "s1").is_err());
}

#[test]
fn the_code_is_redeemed_without_a_secret_for_microsoft() {
    let m = oauthlink::exchange_form(Provider::Microsoft, "c", "http://localhost:1", "v", Some("never-sent"));
    assert!(!m.contains("client_secret") && !m.contains("never-sent"), "{m}");
    assert!(m.contains("code_verifier=v") && m.contains("IMAP.AccessAsUser.All") && !m.contains("Calendars.Read"), "one resource per token request: {m}");
    let g = oauthlink::exchange_form(Provider::Google, "c", "http://127.0.0.1:1", "v", Some("sec"));
    assert!(g.contains("client_secret=sec") && g.contains("code_verifier=v"), "{g}");

    let body = format!(r#"{{"access_token":"a","refresh_token":"r1","expires_in":3600,"id_token":"{}"}}"#, id_token(r#"{"email":"Eric@Outlook.com"}"#));
    let s = oauthlink::signed_in_from(Provider::Microsoft, &body).unwrap();
    assert_eq!((s.email.as_str(), s.refresh_token.as_str()), ("eric@outlook.com", "r1"));
    // Microsoft personal accounts may only say preferred_username.
    let only_username = format!(r#"{{"refresh_token":"r","id_token":"{}"}}"#, id_token(r#"{"preferred_username":"a@hotmail.com"}"#));
    assert_eq!(oauthlink::signed_in_from(Provider::Microsoft, &only_username).unwrap().email, "a@hotmail.com");
    assert!(oauthlink::signed_in_from(Provider::Google, r#"{"access_token":"a"}"#).unwrap_err().contains("no lasting sign-in"));
    let e = oauthlink::signed_in_from(Provider::Google, r#"{"error":"invalid_grant","error_description":"Bad Request"}"#).unwrap_err();
    assert!(e.contains("sign in again"), "{e}");
}

#[test]
fn provider_times_become_the_right_instants() {
    assert_eq!(oauthlink::epoch_of("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(oauthlink::epoch_of("2026-10-05T09:00:00-07:00"), oauthlink::epoch_of("2026-10-05T16:00:00Z"));
    assert_eq!(oauthlink::epoch_of("2026-10-05T16:00:00.0000000"), oauthlink::epoch_of("2026-10-05T16:00:00Z"));
    assert_eq!(oauthlink::epoch_of("2026-10-05T21:30:00+05:30"), oauthlink::epoch_of("2026-10-05T16:00:00Z"));
    assert_eq!(oauthlink::utc_stamp(oauthlink::epoch_of("2026-10-05T16:00:00Z").unwrap() as u64), "2026-10-05T16:00:00Z");
}

fn import(ics: &str) -> Vec<atlas::vformat::Event> {
    atlas::vformat::events_in(ics, &atlas::tz::Zone::utc()).unwrap().0
}

#[test]
fn google_and_outlook_calendars_arrive_as_events() {
    let google = r#"{"items":[
        {"id":"a1","iCalUID":"a1@google.com","status":"confirmed","summary":"Dentist, 2nd floor","location":"Main St",
         "start":{"dateTime":"2026-10-06T09:00:00-07:00"},"end":{"dateTime":"2026-10-06T10:00:00-07:00"}},
        {"id":"a2","status":"confirmed","summary":"Trip","start":{"date":"2026-10-10"},"end":{"date":"2026-10-12"}},
        {"id":"a3","status":"cancelled"}]}"#;
    let evs = import(&oauthlink::ics_from_google(google).unwrap());
    assert_eq!(evs.len(), 2, "the cancelled one is left out");
    assert_eq!(evs[0].summary, "Dentist, 2nd floor");
    assert_eq!(evs[0].location, "Main St");
    assert_eq!(evs[0].start, oauthlink::epoch_of("2026-10-06T16:00:00Z").unwrap());
    assert_eq!(evs[0].end, oauthlink::epoch_of("2026-10-06T17:00:00Z"));
    assert!(evs[1].all_day && evs[1].uid == "google-a2");

    let graph = r#"{"value":[
        {"id":"x","iCalUId":"040000","subject":"Standup","isAllDay":false,"isCancelled":false,
         "start":{"dateTime":"2026-10-07T15:30:00.0000000","timeZone":"UTC"},"end":{"dateTime":"2026-10-07T15:45:00.0000000","timeZone":"UTC"},
         "location":{"displayName":"Teams"}},
        {"id":"y","subject":"Holiday","isAllDay":true,"isCancelled":false,
         "start":{"dateTime":"2026-10-12T00:00:00.0000000","timeZone":"UTC"},"end":{"dateTime":"2026-10-13T00:00:00.0000000","timeZone":"UTC"}},
        {"id":"z","subject":"Gone","isCancelled":true,"start":{"dateTime":"2026-10-08T10:00:00"},"end":{"dateTime":"2026-10-08T11:00:00"}}]}"#;
    let evs = import(&oauthlink::ics_from_graph(graph).unwrap());
    assert_eq!(evs.len(), 2);
    assert_eq!((evs[0].summary.as_str(), evs[0].location.as_str(), evs[0].uid.as_str()), ("Standup", "Teams", "outlook-040000"));
    assert_eq!(evs[0].start, oauthlink::epoch_of("2026-10-07T15:30:00Z").unwrap());
    assert!(evs[1].all_day);
    assert!(oauthlink::ics_from_graph(r#"{"error":{"code":"InvalidAuthenticationToken","message":"Access token has expired."}}"#).unwrap_err().contains("InvalidAuthenticationToken"));
}

/// The network as a script: what each request gets back, and what was asked.
struct Fake {
    asked: RefCell<Vec<String>>,
}

impl Net for Fake {
    fn get(&self, host: &str, path: &str, headers: &[(&str, &str)]) -> Result<Reply, String> {
        let hs: Vec<String> = headers.iter().map(|(k, v)| format!("{k}: {v}")).collect();
        self.asked.borrow_mut().push(format!("GET {host}{path} | {}", hs.join(" | ")));
        let body = if host == "graph.microsoft.com" { r#"{"value":[]}"# } else { r#"{"items":[]}"# };
        Ok(Reply { status: 200, body: body.into(), last_modified: None, retry_after: None })
    }
    fn post_form(&self, host: &str, path: &str, form: &str) -> Result<Reply, String> {
        self.asked.borrow_mut().push(format!("POST {host}{path} {form}"));
        Ok(Reply { status: 200, body: r#"{"access_token":"AT","expires_in":3600}"#.into(), last_modified: None, retry_after: None })
    }
    fn post_json(&self, _: &str, _: &str, _: &[(&str, &str)], _: &str) -> Result<Reply, String> {
        Err("not used".into())
    }
}

#[test]
fn a_signed_in_outlook_calendar_is_read_with_a_fresh_token_in_utc() {
    let net = Fake { asked: RefCell::new(vec![]) };
    let ics = oauthlink::calendar_ics(&net, Provider::Microsoft, "RT", 1_790_000_000).unwrap();
    assert!(ics.starts_with("BEGIN:VCALENDAR"));
    let asked = net.asked.borrow();
    assert!(asked[0].starts_with("POST login.microsoftonline.com/common/oauth2/v2.0/token grant_type=refresh_token&refresh_token=RT"), "{}", asked[0]);
    assert!(asked[0].contains("Calendars.Read") && !asked[0].contains("client_secret"));
    assert!(asked[1].starts_with("GET graph.microsoft.com/v1.0/me/calendarView?startDateTime="), "{}", asked[1]);
    assert!(asked[1].contains("Authorization: Bearer AT") && asked[1].contains(r#"Prefer: outlook.timezone="UTC""#), "{}", asked[1]);
}

#[test]
fn signing_in_keeps_the_token_sealed_and_connects_outlook_mail_and_calendar() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let store = tmp("oauth");
    let mut d = Daemon::new(&c, &p, None, Store::new(store.clone()), Proactive::new(ProactiveConfig::default()));
    d.vault.open("a genuinely long passphrase, not a word", 0, &atlas::vault::VaultConfig::default()).unwrap();

    let page = atlas::hublive::reply(&mut d, Action::Hub(Page::Accounts)).body;
    assert!(page.contains("Sign in with Microsoft</button>"), "{page}");
    assert!(page.contains("Atlas never sees your password"));
    if oauthlink::google_secret().is_none() {
        assert!(page.contains("built without Google's sign-in key"), "a copy without the key says so instead of a dead button");
    }

    let said = atlas::connecting::keep_sign_in(&mut d, &oauthlink::SignedIn { provider: Provider::Microsoft, email: "eric@outlook.com".into(), refresh_token: "RT-1".into() }, 1_790_000_000);
    assert!(said.starts_with("Connected eric@outlook.com"), "{said}");

    assert_eq!(d.vault.get("signin microsoft eric@outlook.com", 1_790_000_000).unwrap(), "RT-1", "sealed in the vault");
    let kept: Vec<atlas::mail::Account> = Store::new(store.clone()).load(atlas::daemon::CONNECTED_ACCOUNTS);
    assert_eq!(kept.len(), 1);
    assert!(kept[0].oauth && kept[0].client_id == oauthlink::MICROSOFT_CLIENT_ID && kept[0].password_from_vault == "signin microsoft eric@outlook.com");
    assert_eq!(kept[0].imap_host, "outlook.office365.com");
    let links: Vec<connect::CalendarLink> = Store::new(store.clone()).load(connect::CALENDAR_LINKS);
    assert_eq!(links[0].url, "oauth:microsoft:eric@outlook.com");
    assert_eq!(links[0].name, "Outlook Calendar (eric@outlook.com)");
    assert!(!std::fs::read_to_string(store.join(format!("{}.json", connect::CALENDAR_LINKS))).unwrap_or_default().contains("RT-1"), "the token is never in the store");

    let page = atlas::hublive::reply(&mut d, Action::Hub(Page::Accounts)).body;
    assert!(page.contains("Connected eric@outlook.com: Outlook mail and calendar"), "{page}");
    assert!(page.contains("<b>Outlook Calendar (eric@outlook.com)</b> calendar"));

    // The calendar off: the mail still uses the token, so it stays.
    atlas::hublive::reply(&mut d, Action::HubPost { path: "/hub/connect".into(), fields: fields(&[("what", "disconnect"), ("kind", "calendar"), ("id", "oauth:microsoft:eric@outlook.com")]) });
    assert!(d.vault.get("signin microsoft eric@outlook.com", 1_790_000_000).is_ok());
    // The mail off too: now the token goes.
    atlas::hublive::reply(&mut d, Action::HubPost { path: "/hub/connect".into(), fields: fields(&[("what", "disconnect"), ("kind", "mail"), ("id", "eric@outlook.com")]) });
    assert!(d.vault.get("signin microsoft eric@outlook.com", 1_790_000_000).is_err(), "nothing left behind");

    // An unknown provider button does nothing.
    let r = atlas::hublive::reply(&mut d, Action::HubPost { path: "/hub/connect".into(), fields: fields(&[("what", "oauth"), ("provider", "yahoo")]) });
    assert!(r.body.contains("isn%27t+wired") || r.body.contains("isn't+wired") || r.body.contains("wired"), "{}", r.body);
}
