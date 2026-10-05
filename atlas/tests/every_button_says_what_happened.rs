//! Every button on the hub says what it did, and none of them holds Atlas up
//! (27 Sep 2026).
//!
//! Before this: `?said=` reached only the sixteen pages that read their
//! query, so a button anywhere else came back to a page that looked the same
//! whether or not anything happened; several handlers threw their sentence
//! away; a form missing a field showed "That isn't a page in Atlas"; and the
//! buttons whose work goes over the network (sending a document, adding a
//! friend, a phone's code) ran inside the request, on the daemon's thread,
//! so the whole of Atlas waited on Tor or Tailscale.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::hub::{self, Page};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::server::{route, Action, Reply, Request, Server, ServerConfig};
use atlas::store::Store;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}
fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}
fn install(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("atlas-buttons-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("data/state")).unwrap();
    std::fs::create_dir_all(root.join("peers")).unwrap();
    root
}
fn daemon<'a>(root: &Path, c: &'a Config, p: &'a MockPlatform) -> Daemon<'a> {
    let mut d = Daemon::new(c, p, None, Store::new(root.join("data/state")), Proactive::new(ProactiveConfig::default()));
    d.peer_dir = root.join("peers");
    d
}

/// Follow a redirect as the browser does: the fragment stays in the browser,
/// the query goes to the page.
fn follow(d: &mut Daemon, r: &Reply) -> String {
    assert_eq!(r.status, 303, "a button should send you back to a page: {}", r.body);
    let to = r.body.split('#').next().unwrap();
    let (path, q) = to.split_once('?').unwrap_or((to, ""));
    let page = hub::route(path).unwrap_or_else(|| panic!("sent to {path}, which isn't a page"));
    let action = if q.is_empty() { Action::Hub(page) } else { Action::HubQ(page, q.to_string()) };
    atlas::hublive::reply(d, action).body
}

/// Pressed, followed, and said.
fn says(d: &mut Daemon, action: Action, words: &str) -> String {
    let r = atlas::hublive::reply(d, action.clone());
    assert!(r.body.contains("said="), "{action:?} came back without saying what it did: {}", r.body);
    let html = follow(d, &r);
    assert!(html.contains("class=notice"), "{action:?}: the page shows nothing about it");
    assert!(html.contains(words), "{action:?} should have said {words:?}: {html}");
    html
}

#[test]
fn each_button_comes_back_to_its_page_saying_what_it_did() {
    let (c, p) = (cfg(), plat());
    let root = install("said");
    let mut d = daemon(&root, &c, &p);

    // 28 Sep 2026: Pause turns the microphone off too, and says so.
    says(&mut d, Action::Pause(true), "Paused. Nothing new starts, and the microphone is off, until you resume.");
    says(&mut d, Action::Pause(false), "Carrying on. The microphone is back on.");
    says(&mut d, Action::Account(atlas::accounts::Change::Note("github".into())), "Noted github.");
    says(&mut d, Action::Account(atlas::accounts::Change::Forget("github".into())), "Forgot github.");
    says(&mut d, Action::Account(atlas::accounts::Change::Forget("github".into())), "Nothing changed.");
    says(&mut d, Action::RevokeAccess("mybank.com".into()), "nothing to take away");
    says(&mut d, Action::RevokeAllAccess, "nothing to take away");
    let id = d.tray.hand("https://a.example", &atlas::earned::Space::Personal, "phone", 1).unwrap();
    says(&mut d, Action::TrayDone(id), "Done with it.");
    assert!(d.tray.open().is_empty(), "said done and left it open");
    assert!(d.accounts.get("github").is_none(), "said forgotten and kept it");
    says(&mut d, Action::Friend { what: "decline".into(), who: "Nobody".into(), link: String::new() }, "no friend request from Nobody");
    says(&mut d, Action::Friend { what: "nonsense".into(), who: String::new(), link: String::new() }, "isn&#39;t wired to anything");
    says(&mut d, Action::Friend { what: "add".into(), who: String::new(), link: "not a link".into() }, "");
    says(&mut d, Action::AddOn { what: "share".into(), id: "nothing-here".into(), key: "Friends".into(), sha: String::new() }, "");
    says(&mut d, Action::SyncJoin { code: "ABC".into(), device: "laptop".into() }, "");
    says(&mut d, Action::HubBack(Page::Accounts, "Type the site's name first, then press the button.".into()), "Type the site");
    // A form posted to a path nothing answers.
    let r = atlas::hublive::reply(&mut d, Action::HubPost { path: "/hub/nowhere".into(), fields: vec![] });
    assert!(follow(&mut d, &r).contains("isn&#39;t wired to anything"));
}

#[test]
fn a_setting_changed_goes_back_to_that_setting_and_says_so_even_when_refused() {
    let (c, p) = (cfg(), plat());
    let root = install("setting");
    let mut d = daemon(&root, &c, &p);
    // A value it won't take is said, not swallowed, and nothing is written.
    let r = atlas::hublive::reply(&mut d, Action::HubSet { key: "no.such.setting".into(), value: "on".into() });
    assert!(r.body.starts_with("/hub/settings?said=") && r.body.ends_with("#set-no.such.setting"), "{}", r.body);
    let html = follow(&mut d, &r);
    assert!(html.contains("class=notice") && html.contains("no setting called no.such.setting"), "{html}");
    // Every setting's row can be pointed at.
    assert!(html.contains("id='set-mail.enabled'"), "the rows have no anchors to come back to");
}

#[test]
fn a_form_missing_what_it_needs_goes_back_saying_so_rather_than_to_a_fault_page() {
    let post = |path: &str, body: &str| Request {
        method: "POST".into(),
        path: path.into(),
        query: String::new(),
        token: Some("x".repeat(24)),
        token_from_url: false,
        body: body.into(),
    };
    for (path, body, page) in [
        ("/hub/accounts", "what=note&site=", Page::Accounts),
        ("/hub/access/revoke", "", Page::Access),
        ("/hub/pause", "", Page::Now),
        ("/hub/dash", "what=up", Page::Dashboard),
        ("/hub/addons", "what=on", Page::AddOns),
        ("/hub/edits", "file=x", Page::Edits),
        ("/hub/set", "value=on", Page::Settings),
        ("/hub/tray", "", Page::Dashboard),
        ("/hub/friends", "", Page::Friends),
        ("/hub/groups", "", Page::Groups),
        ("/hub/sync", "what=explode", Page::Sync),
    ] {
        match route(&post(path, body)) {
            Some(Action::HubBack(p, said)) => {
                assert_eq!(p, page, "{path}");
                assert!(said.ends_with('.') && !said.is_empty(), "{path}: {said}");
            }
            other => panic!("{path} with {body:?} routed to {other:?}"),
        }
    }
    // The empty "Track it" is told what's missing.
    let Some(Action::HubBack(_, said)) = route(&post("/hub/accounts", "what=note&site=")) else { unreachable!() };
    assert!(said.contains("site's name"), "{said}");
}

#[test]
fn every_page_hears_what_a_button_said_not_only_the_ones_that_read_their_address() {
    let get = |path: &str, query: &str| Request {
        method: "GET".into(),
        path: path.into(),
        query: query.into(),
        token: Some("x".repeat(24)),
        token_from_url: false,
        body: String::new(),
    };
    assert!(!Page::Access.reads_query());
    assert_eq!(route(&get("/hub/access", "said=Done")), Some(Action::HubQ(Page::Access, "said=Done".into())));
    assert_eq!(route(&get("/hub/now", "job=3&t=secret")), Some(Action::HubQ(Page::Now, "job=3".into())));
    // Anything else in the address still isn't read by a page that doesn't.
    assert_eq!(route(&get("/hub/access", "x=1")), Some(Action::Hub(Page::Access)));
}

#[test]
fn a_friend_link_never_goes_in_an_address_and_is_shown_once() {
    let (c, p) = (cfg(), plat());
    let root = install("link");
    let mut d = daemon(&root, &c, &p);
    let r = atlas::hublive::reply(&mut d, Action::Friend { what: "link".into(), who: String::new(), link: String::new() });
    assert_eq!(r.status, 303, "pressing it again on refresh would make another link: {}", r.body);
    assert!(!r.body.contains("atlas") || r.body == "/hub/friends", "a link in the address lands in history: {}", r.body);
    if r.body == "/hub/friends" {
        // Made: on the page once, then gone.
        let first = follow(&mut d, &r);
        // The QR picture needs an encoder this machine may not have (CI didn't);
        // the link itself is what must be there.
        assert!(first.contains("class=qr") || first.contains("<code") || first.contains("atlas-friend:"), "the link isn't shown: {first}");
        let again = atlas::hublive::reply(&mut d, Action::Hub(Page::Friends)).body;
        assert_ne!(first, again, "the link is shown again");
    } else {
        // This machine couldn't make one (no door): it says why, and the
        // reason is all the address carries.
        assert!(r.body.starts_with("/hub/friends?said="), "{}", r.body);
    }
}

#[test]
fn sending_a_document_comes_straight_back_and_the_page_follows_the_send() {
    use atlas::kin::{Contact, Pairings};
    let (c, p) = (cfg(), plat());
    let root = install("slowsend");
    // Sam's Atlas: takes the connection, then says nothing for two seconds.
    let far = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = far.local_addr().unwrap().port();
    let sleeper = std::thread::spawn(move || {
        if let Ok((s, _)) = far.accept() {
            std::thread::sleep(std::time::Duration::from_secs(2));
            drop(s);
        }
    });
    let mut pairings = Pairings::default();
    pairings.contacts.push(Contact { name: "Sam".into(), host: "127.0.0.1".into(), port, token: "t".repeat(24) });
    pairings.save(&root.join("peers")).unwrap();
    let mut d = daemon(&root, &c, &p);
    let id = d.tray.hand("https://example.com/plan", &atlas::earned::Space::Personal, "the hub", 10).unwrap();

    let started = std::time::Instant::now();
    let fields = [("what", "send"), ("id", id.to_string().as_str()), ("who", "Sam")]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let r = atlas::hublive::reply(&mut d, Action::HubPost { path: "/hub/documents".into(), fields });
    assert!(started.elapsed().as_millis() < 1500, "the form waited for Sam's Atlas: {:?}", started.elapsed());
    assert!(r.body.contains("job="), "{}", r.body);

    // While it's going: said, and the page looks again by itself.
    let running = follow(&mut d, &r);
    assert!(running.contains("Sending &quot;") && running.contains("to Sam"), "{running}");
    assert!(running.contains("<meta http-equiv=refresh content=2>"), "the page won't look again");

    d.errands_done_for_test();
    sleeper.join().unwrap();
    let done = follow(&mut d, &r);
    assert!(!done.contains("http-equiv=refresh"), "it keeps refreshing once it's done");
    assert!(done.contains("class=notice") && done.contains("couldn&#39;t reach Sam"), "{done}");
    assert!(!done.contains("os error"), "the network's own words on the page: {done}");
    // Shown once: the page without the job doesn't say it again.
    let later = atlas::hublive::reply(&mut d, Action::Hub(Page::Documents)).body;
    assert!(!later.contains("couldn&#39;t reach Sam"), "said twice");
    assert!(d.tray.items.iter().find(|i| i.id == id).unwrap().shared.is_empty(), "logged as sent when it wasn't");
}

#[test]
fn a_share_sheet_arrival_is_taken_once_and_a_reload_does_not_add_it_again() {
    let (c, p) = (cfg(), plat());
    let root = install("share");
    let mut d = daemon(&root, &c, &p);
    let r = atlas::hublive::reply(&mut d, Action::HubQ(Page::Give, "url=https%3A%2F%2Fexample.com%2Fx".into()));
    assert_eq!(r.status, 303, "the share's own address stays in the browser, and a reload adds it again");
    assert!(r.body.starts_with("/hub/give?said="), "{}", r.body);
    assert_eq!(d.tray.items.len(), 1);
    let html = follow(&mut d, &r);
    assert!(html.contains("Sent to Atlas"), "{html}");
    assert_eq!(d.tray.items.len(), 1, "following it added it again");
}

/// Send `body` to `path` on a real server, and what came back.
fn exchange(path: &str, body: &str) -> (Option<Action>, String) {
    let token = "t".repeat(32);
    let cfg = ServerConfig { enabled: true, port: 0, ..ServerConfig::default() };
    let server = Server::bind(&cfg, &token).unwrap();
    let port = server.port();
    let (path, body) = (path.to_string(), body.to_string());
    let client = std::thread::spawn(move || {
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
        let head = format!(
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Atlas-Token: {token}\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        s.write_all(head.as_bytes()).unwrap();
        let _ = s.write_all(body.as_bytes());
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    });
    let got = server.serve_once(&mut |_| Reply::ok("{}")).unwrap();
    (got, client.join().unwrap())
}

#[test]
fn a_phones_six_weeks_of_calendar_is_taken_in_one_go() {
    let events: Vec<atlas::calendar::PhoneEvent> = (0..300)
        .map(|i| atlas::calendar::PhoneEvent {
            key: format!("E621E1F8-C36C-495A-93FC-0C247A3E6E5F:{i}"),
            title: format!("Planning session with the Northwind team, week {i}"),
            start: 1_790_000_000 + i * 3600,
            end: 1_790_000_000 + i * 3600 + 1800,
            all_day: false,
            place: Some("Conference room B, second floor".into()),
        })
        .collect();
    let batch = serde_json::json!({ "from": 1_790_000_000u64, "to": 1_793_700_000u64, "events": events }).to_string();
    assert!(batch.len() > 16 * 1024, "not a test of the old limit: {} bytes", batch.len());
    let (got, answer) = exchange("/hub/calendar/phone", &batch);
    assert!(answer.starts_with("HTTP/1.1 200"), "{}", &answer[..answer.len().min(200)]);
    match got {
        Some(Action::PhoneCalendar(b)) => {
            let read: atlas::calendar::PhoneBatch = serde_json::from_str(&b).unwrap();
            assert_eq!(read.events.len(), 300);
        }
        other => panic!("{other:?}"),
    }
    // Still a limit.
    let r = |p: &str| Request { method: "POST".into(), path: p.into(), query: String::new(), token: None, token_from_url: false, body: String::new() };
    assert_eq!(atlas::server::body_cap(&r("/hub/calendar/phone"), 16 * 1024, 28 << 20), atlas::server::CALENDAR_BODY);
    assert_eq!(atlas::server::body_cap(&r("/hub/give"), 16 * 1024, 28 << 20), 16 * 1024);
}

#[test]
fn too_much_sent_from_a_hub_form_is_a_page_with_the_way_back() {
    let text = format!("text={}", "a".repeat(40 * 1024));
    let (got, answer) = exchange("/hub/give", &text);
    assert!(got.is_none());
    assert!(answer.starts_with("HTTP/1.1 413"), "{}", &answer[..answer.len().min(200)]);
    assert!(answer.contains("text/html") && answer.contains("href='/hub/give'"), "{answer}");
    assert!(!answer.contains("{\"error\""), "a bare error with no way back");
}

#[test]
fn names_that_are_not_plain_ascii_never_crash_anything() {
    use atlas::earned::Space;
    // A business name starting with an accented letter, first seen inside a
    // longer word: stepping on one byte from there split "É".
    assert_eq!(
        atlas::calendar::space_for_request("book time with xÉlan and Élan", &["Élan".to_string()]),
        Space::Business("Élan".into())
    );
    assert_eq!(atlas::calendar::space_for_request("lunch at 李记", &["李".to_string()]), Space::Personal);
    let people = [
        atlas::linkage::Contact { name: "Zoë Li".into(), email: String::new(), phone: String::new() },
        atlas::linkage::Contact { name: "Zoë Lin".into(), email: String::new(), phone: String::new() },
        atlas::linkage::Contact { name: "李小龙".into(), email: String::new(), phone: String::new() },
    ];
    assert!(atlas::linkage::candidate_pairs(&people).contains(&(0, 1)));
}

#[test]
fn the_settings_only_window_actually_keeps_a_change() {
    let dir = install("settingsonly").join("config");
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = atlas::settings::registry(&atlas::voice::ToolsConfig::default());
    let said = s.set_and_keep("mail.enabled", "on", &dir);
    assert!(!said.contains("couldn't"), "{said}");
    let kept = atlas::preferences::Preferences::load(&dir);
    assert_eq!(kept.chosen.get("mail.enabled").map(String::as_str), Some("on"), "said it was on and wrote nothing");
    // A value it won't take writes nothing.
    let said = s.set_and_keep("no.such.setting", "on", &dir);
    assert!(said.contains("no setting called"), "{said}");
    assert!(!atlas::preferences::Preferences::load(&dir).chosen.contains_key("no.such.setting"));
}

#[test]
fn what_a_button_said_comes_back_letter_for_letter() {
    // "—" went out as `%14` and "é" raw until 27 Sep 2026.
    let said = "Sent to Atlas. Reading it now — I'll tell you what I find about Zoë’s café…";
    let r = hub::back_with("/hub/give", "", said);
    assert!(r.body.is_ascii(), "{}", r.body);
    let q = r.body.split_once('?').unwrap().1;
    assert_eq!(hub::form_fields(q).into_iter().find(|(k, _)| k == "said").unwrap().1, said);
    // Nothing from a form can end the header it goes into.
    let r = hub::back_with("/hub/settings#set-x\r\nSet-Cookie: a=b", "", "ok");
    assert!(!r.body.contains('\r') && !r.body.contains('\n') && !r.body.contains(' '), "{}", r.body);
}
