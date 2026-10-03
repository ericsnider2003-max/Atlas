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
