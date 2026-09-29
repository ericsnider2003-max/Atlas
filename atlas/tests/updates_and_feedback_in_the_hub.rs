//! The hub's Updates and Feedback pages (OPEN_GAPS 8.2, 8.14), through the
//! running Atlas's own hub answers, and the hub's text fields each carrying a
//! decided `autocomplete` (WCAG 1.3.5, OPEN_GAPS P.6).

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::feedback::{compose_feedback, feedback_inbox, feedback_outbox, heard_feedback, answers_out};
use atlas::groups::{GroupState, Groups, Held, Signed};
use atlas::hub::{self, Page};
use atlas::kin::{Contact, Pairings, Peer};
use atlas::peerkey::Identity;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::server::Action;
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}
fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn install(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("atlas-hubupd-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("data/state")).unwrap();
    std::fs::create_dir_all(root.join("peers")).unwrap();
    root
}

fn in_channel(root: &Path, owner: &str) {
    let state = GroupState {
        format: 1,
        group_id: "og-rel".into(),
        owner: owner.into(),
        name: "Atlas releases".into(),
        version: 1,
        seats: vec![],
        release_channel: true,
        delegates: vec![],
    };
    let mut g = Groups::default();
    g.held.insert("og-rel".into(), Held { signed: Signed { state: String::new(), signature: String::new(), signer: String::new() }, state });
    g.save(&Store::new(root.join("data/state"))).unwrap();
    let me = Identity::load_or_create(&root.join("peers")).unwrap().public();
    if owner != me {
        let mut p = Pairings::default();
        let mut e = Peer::new("Eric", "hub-token-00000000000000000");
        e.key = Some(owner.into());
        p.peers.push(e);
        p.contacts.push(Contact { name: "Eric".into(), host: "127.0.0.1".into(), port: 1, token: "hub-token-00000000000000000".into() });
        p.save(&root.join("peers")).unwrap();
    }
}

fn daemon<'a>(root: &Path, c: &'a Config, p: &'a MockPlatform) -> Daemon<'a> {
    let mut d = Daemon::new(c, p, None, Store::new(root.join("data/state")), Proactive::new(ProactiveConfig::default()));
    d.peer_dir = root.join("peers");
    d
}

fn post(d: &mut Daemon, path: &str, fields: &[(&str, &str)]) -> String {
    let fields = fields.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let r = atlas::hublive::reply(d, Action::HubPost { path: path.into(), fields });
    assert_eq!(r.status, 303, "a form should send you back to the page: {}", r.body);
    // Follow it, as the browser would: the page, with what happened said once.
    let (page, q) = r.body.split_once('?').unwrap_or((&r.body, ""));
    let p = hub::route(page).expect("sent back to a real page");
    atlas::hublive::reply(d, Action::HubQ(p, q.to_string())).body
}

#[test]
fn both_pages_sit_in_the_navigation_route_and_are_findable() {
    for p in [Page::Updates, Page::Feedback] {
        assert!(hub::NAV.iter().any(|(_, ps)| ps.contains(&p)), "{} isn't in the sidebar", p.label());
        assert_eq!(hub::route(p.href()), Some(p));
        assert!(p.reads_query(), "{} would lose what happened after a form", p.label());
    }
    let all = atlas::palette::catalogue();
    let recent = atlas::palette::Recent::default();
    let hits = atlas::palette::find(&all, "updates", &recent);
    assert!(hits.iter().any(|e| e.label == "Check for updates"));
    let hits = atlas::palette::find(&all, "report a bug", &recent);
    assert!(hits.iter().any(|e| e.label == "Report a problem with Atlas"));
}

#[test]
fn the_updates_page_says_what_this_is_and_going_back_asks_first() {
    let (c, p) = (cfg(), plat());
    let root = install("updates");
    let mut d = daemon(&root, &c, &p);
    let html = atlas::hublive::reply(&mut d, Action::Hub(Page::Updates)).body;
    assert!(html.contains(&format!("Atlas {}", atlas::upgrade::version())), "which Atlas this is isn't said");
    assert!(html.contains("No earlier version is kept here"), "{html}");
    assert!(html.contains("value=default checked"), "the usual mode isn't shown as chosen");
    // A kept previous build: a link to the question, never a one-press undo.
    std::fs::write(atlas::upgrade::keep_old_at(&root, "0.0.9"), b"old").unwrap();
    let html = atlas::hublive::reply(&mut d, Action::Hub(Page::Updates)).body;
    assert!(html.contains("Go back to Atlas 0.0.9…") && !html.contains("value=undo-confirmed"), "going back is one press");
    let html = atlas::hublive::reply(&mut d, Action::HubQ(Page::Updates, "confirm=undo".into())).body;
    assert!(html.contains("value=undo-confirmed") && html.contains("Keep Atlas"), "the question isn't asked");
    // Choosing how updates go in is kept.
    let html = post(&mut d, "/hub/updates", &[("what", "mode"), ("mode", "ask")]);
    assert!(html.contains("ask before installing each update.") && html.contains("value=ask checked"), "{html}");
    // Kept where the update courier reads it, not just echoed on the page.
    let store = Store::new(root.join("data/state"));
    assert_eq!(atlas::update_apply::chosen_mode(&store), Some(atlas::update_apply::AutoUpdate::Ask));
    post(&mut d, "/hub/updates", &[("what", "mode"), ("mode", "default")]);
    assert_eq!(atlas::update_apply::chosen_mode(&store), None, "the usual mode wasn't restored");
    let html = post(&mut d, "/hub/updates", &[("what", "install")]);
    assert!(html.contains("no update waiting."), "{html}");
}

#[test]
fn a_friend_writes_feedback_sees_it_exactly_and_it_goes_only_on_send() {
    let (c, p) = (cfg(), plat());
    let root = install("fbfriend");
    in_channel(&root, &Identity::from_seed_for_test([51; 32]).public());
    let store = Store::new(root.join("data/state"));
    let mut d = daemon(&root, &c, &p);
    let html = atlas::hublive::reply(&mut d, Action::Hub(Page::Feedback)).body;
    assert!(html.contains("Show me what will go to Eric") && html.contains("<label for=fbwords>"), "{html}");
    assert!(!html.contains("name=attach"), "a failure is offered when nothing failed here");
    let html = post(&mut d, "/hub/feedback", &[("what", "preview"), ("words", "The calendar shows yesterday")]);
    assert!(html.contains("exactly what will go to Eric") && html.contains("The calendar shows yesterday"), "{html}");
    assert!(feedback_outbox(&store).is_empty(), "sent on preview");
    let html = post(&mut d, "/hub/feedback", &[("what", "send")]);
    assert!(html.contains("Sending it to Eric"), "{html}");
    assert_eq!(feedback_outbox(&store).len(), 1);
    let html = post(&mut d, "/hub/feedback", &[("what", "send")]);
    assert!(html.contains("nothing waiting to send"), "a second press sent it twice: {html}");
    assert_eq!(feedback_outbox(&store).len(), 1);
    assert!(html.contains("What you've sent") && html.contains("The calendar shows yesterday"));
}

#[test]
fn the_releaser_answers_feedback_and_holds_a_reported_build_from_the_hub() {
    let (c, p) = (cfg(), plat());
    let root = install("fbowner");
    let me = Identity::load_or_create(&root.join("peers")).unwrap().public();
    in_channel(&root, &me);
    let store = Store::new(root.join("data/state"));
    let failure = atlas::update_apply::FailureReport {
        version: "9.2.0".into(),
        sha256: "dd".repeat(32),
        platform: "windows-x86_64".into(),
        stage: "probation".into(),
        reasons: vec!["3 starts in a row never got through".into()],
        ..Default::default()
    };
    let f = compose_feedback("The hub goes blank after the update", Some(failure), 5).unwrap();
    heard_feedback(&store, "Priya", &serde_json::to_string(&f).unwrap()).unwrap();
    let mut d = daemon(&root, &c, &p);
    let html = atlas::hublive::reply(&mut d, Action::Hub(Page::Feedback)).body;
    assert!(html.contains("1. Priya") && html.contains("update failure attached") && html.contains("Answer Priya"), "{html}");
    let html = post(&mut d, "/hub/feedback", &[("what", "answer"), ("n", "1"), ("status", "fixed"), ("version", ""), ("note", "")]);
    assert!(html.contains("needs the version"), "{html}");
    let html = post(&mut d, "/hub/feedback", &[("what", "answer"), ("n", "1"), ("status", "fixing"), ("note", "on it")]);
    assert!(html.contains("being fixed") && html.contains("Priya"), "{html}");
    assert_eq!(answers_out(&store).len(), 1);
    assert_eq!(feedback_inbox(&store)[0].status, atlas::feedback::FeedbackStatus::Fixing);
    // The attached failure is on the Updates page, with a hold only the releaser has.
    let html = atlas::hublive::reply(&mut d, Action::Hub(Page::Updates)).body;
    assert!(html.contains("Updates that failed") && html.contains("Stop handing out Atlas 9.2.0"), "{html}");
    let html = post(&mut d, "/hub/updates", &[("what", "hold"), ("sha", &"dd".repeat(32))]);
    let tail = &html[html.find("Updates that failed").unwrap_or(0)..];
    assert!(html.contains("Held"), "{html}");
    assert!(!tail.contains("Stop handing out Atlas 9.2.0"), "{tail}");
    assert!(atlas::update_apply::is_halted(store.root(), &"dd".repeat(32)));
    let html = post(&mut d, "/hub/updates", &[("what", "hold"), ("sha", &"ee".repeat(32))]);
    assert!(html.contains("a build anyone has reported"), "{html}");
}

#[test]
fn a_friend_cannot_hold_the_releasers_builds() {
    let (c, p) = (cfg(), plat());
    let root = install("fbnohold");
    in_channel(&root, &Identity::from_seed_for_test([52; 32]).public());
    let mut d = daemon(&root, &c, &p);
    let html = post(&mut d, "/hub/updates", &[("what", "hold"), ("sha", &"dd".repeat(32))]);
    assert!(html.contains("Only the person who sends Atlas out"), "{html}");
}

/// WCAG 1.3.5: a field that collects your own details says so; one that
/// doesn't (a client's email, a task, a note) says `off`, so the browser
/// doesn't offer your own name and address in it. Every text field in the
/// hub's source carries that decision.
#[test]
fn every_text_field_in_the_hub_says_what_it_is_for() {
    for file in ["src/hub.rs", "src/hubpages.rs", "src/hublive.rs"] {
        let src = crate::common::read_source_path(file).unwrap();
        for (i, tag) in src.match_indices("<input").map(|(i, _)| (i, &src[i..src[i..].find('>').map(|e| i + e).unwrap_or(src.len())])) {
            let t = tag.replace('\\', "");
            let exempt = ["hidden", "radio", "checkbox", "file", "range", "number", "date", "time"]
                .iter()
                .any(|k| t.contains(&format!("type={k}")) || t.contains(&format!("type=\"{k}\"")) || t.contains(&format!("type='{k}'")));
            assert!(exempt || t.contains("autocomplete"), "{file} at byte {i}: a text field with no autocomplete decision: {t}");
        }
        for (i, _) in src.match_indices("<textarea") {
            let tag = &src[i..src[i..].find('>').map(|e| i + e).unwrap_or(src.len())];
            assert!(tag.contains("autocomplete"), "{file} at byte {i}: {tag}");
        }
    }
}

// ---- Partners: online or not; Documents: sending one, and the log ---------

fn peers_of(root: &Path) -> PathBuf {
    root.join("peers")
}

#[test]
fn partners_show_whether_their_atlas_is_up_not_only_that_it_is_paired() {
    let (c, p) = (cfg(), plat());
    let root = install("partners");
    let store = Store::new(root.join("data/state"));
    let mut pairings = Pairings::default();
    for name in ["Sam", "Maya", "Lee"] {
        pairings.peers.push(Peer::new(name, &format!("{name}-tok-000000000000000000")));
        pairings.contacts.push(Contact { name: name.into(), host: "127.0.0.1".into(), port: 1, token: format!("{name}-tok-000000000000000000") });
    }
    pairings.save(&peers_of(&root)).unwrap();
    let mut roster = atlas::roster::Roster::load(&store);
    for name in ["Sam", "Maya", "Lee"] {
        roster.add("Northwind", name, &pairings).unwrap();
    }
    roster.save(&store).unwrap();
    let now = atlas::store::now();
    let mut heard = std::collections::BTreeMap::new();
    heard.insert("sam".to_string(), now - 60);
    heard.insert("maya".to_string(), now - 3 * 86_400);
    store.save(atlas::kin::REACHED, &heard).unwrap();
    let mut d = daemon(&root, &c, &p);
    let html = atlas::hublive::reply(&mut d, Action::Hub(Page::Partners)).body;
    let row = |n: &str| html[html.find(&format!("<th scope=row>{n}</th>")).unwrap()..].split("</tr>").next().unwrap().to_string();
    assert!(row("Sam").contains("Online"), "{}", row("Sam"));
    assert!(row("Maya").contains("Offline") && row("Maya").contains("last heard"), "{}", row("Maya"));
    assert!(row("Lee").contains("not heard from yet"), "{}", row("Lee"));
    // Online is the last 15 minutes, measured, not a label: one second past
    // it is Offline, and what the daemon keeps is what it loaded.
    heard.insert("lee".to_string(), now - atlas::kin::ONLINE_SECS - 1);
    store.save(atlas::kin::REACHED, &heard).unwrap();
    let mut d = daemon(&root, &c, &p);
    let html = atlas::hublive::reply(&mut d, Action::Hub(Page::Partners)).body;
    let lee = html[html.find("<th scope=row>Lee</th>").unwrap()..].split("</tr>").next().unwrap().to_string();
    assert!(lee.contains("Offline"), "{lee}");
    assert_eq!(atlas::kin::Reached::load(&store).last("sam"), Some(now - 60));
}

#[test]
fn a_paired_atlas_at_the_door_is_noted_as_heard_from() {
    let r = std::sync::Arc::new(atlas::kin::Reached::default());
    let sam = Peer::new("Sam", "sam-door-token-000000000000");
    let l = atlas::server::SignalListener::bind(0, vec![sam]).unwrap();
    l.note_reached(r.clone());
    let port = l.port();
    let h = std::thread::spawn(move || {
        atlas::http::post_json_with_token(&format!("127.0.0.1:{port}"), "/signal", "{\"intent\":\"ping\"}", "sam-door-token-000000000000", std::time::Duration::from_secs(3))
    });
    let start = std::time::Instant::now();
    while !h.is_finished() && start.elapsed().as_secs() < 5 {
        let _ = l.poll_once(1);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let _ = h.join();
    assert!(r.last("sam").is_some(), "Sam came to the door and wasn't noted");
    assert!(r.last("maya").is_none());
}

#[test]
fn a_document_sent_from_the_hub_reaches_them_and_stops_showing_private() {
    use atlas::server::SignalListener;
    let (c, p) = (cfg(), plat());
    let (eric, sam) = (install("doc-eric"), install("doc-sam"));
    let eric_key = Identity::load_or_create(&peers_of(&eric)).unwrap().public();
    let sam_key = Identity::load_or_create(&peers_of(&sam)).unwrap().public();
    // Sam's door, knowing Eric.
    let token = "eric-sam-doc-token-000000000000";
    let mut sp = Pairings::default();
    let mut ep = Peer::new("Eric", token);
    ep.key = Some(eric_key.clone());
    sp.peers.push(ep.clone());
    sp.save(&peers_of(&sam)).unwrap();
    let l = SignalListener::bind(0, vec![ep]).unwrap();
    l.serve_sealed(Identity::load_or_create(&peers_of(&sam)).unwrap());
    let port = l.port();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop2 = stop.clone();
    let door = std::thread::spawn(move || {
        let mut got = Vec::new();
        while !stop2.load(std::sync::atomic::Ordering::Relaxed) {
            if let Some(a) = l.poll_once(20) {
                got.push(a);
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        got
    });
    // Eric, paired with Sam there.
    let mut e = Pairings::default();
    let mut s = Peer::new("Sam", token);
    s.key = Some(sam_key);
    e.peers.push(s);
    e.contacts.push(Contact { name: "Sam".into(), host: "127.0.0.1".into(), port, token: token.into() });
    e.save(&peers_of(&eric)).unwrap();
    let mut d = daemon(&eric, &c, &p);
    let id = d.tray.hand("https://example.com/quarterly-plan", &atlas::earned::Space::Personal, "the hub", 10).unwrap();
    let html = atlas::hublive::reply(&mut d, Action::Hub(Page::Documents)).body;
    assert!(html.contains("Private") && html.contains("<option>Sam</option>"), "{html}");
    // 27 Sep 2026: the send runs on the crew, not inside the request (a file
    // over Tor held all of Atlas). The form comes straight back to the page
    // with the job's number; the page says how it's going until it's done.
    let fields = [("what", "send"), ("id", id.to_string().as_str()), ("who", "Sam")]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let r = atlas::hublive::reply(&mut d, Action::HubPost { path: "/hub/documents".into(), fields });
    assert_eq!(r.status, 303, "{}", r.body);
    assert!(r.body.contains("job="), "the send isn't a job the page can follow: {}", r.body);
    let q = r.body.split_once('?').unwrap().1.to_string();
    d.errands_done_for_test();
    let html = atlas::hublive::reply(&mut d, Action::HubQ(Page::Documents, q)).body;
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let heard = door.join().unwrap();
    assert!(html.contains("Sent") && html.contains("to Sam"), "{html}");
    assert!(html.contains("Sent to Sam"), "the share isn't logged on the document: {html}");
    assert!(
        heard.iter().any(|a| matches!(a, atlas::kin::Arrived::Handoff(h) if h.what.contains("quarterly-plan"))),
        "the document never arrived at Sam's door"
    );
    let item = d.tray.items.iter().find(|i| i.id == id).unwrap();
    assert_eq!(item.shared.len(), 1);
}

/// The pages as the browser gets them, written to `ATLAS_AXE_OUT` for an
/// accessibility checker (axe-core in a real browser, as 26 Sep's pass did).
/// `ATLAS_AXE_OUT=/tmp/pages cargo test --test all render_the_new_pages -- --ignored`
#[test]
#[ignore = "writes pages out for an outside checker"]
fn render_the_new_pages_for_an_accessibility_check() {
    let out = PathBuf::from(std::env::var("ATLAS_AXE_OUT").expect("ATLAS_AXE_OUT"));
    std::fs::create_dir_all(&out).unwrap();
    let (c, p) = (cfg(), plat());
    let root = install("axe");
    let me = Identity::load_or_create(&root.join("peers")).unwrap().public();
    in_channel(&root, &me);
    let store = Store::new(root.join("data/state"));
    let failure = atlas::update_apply::FailureReport { version: "9.2.0".into(), sha256: "dd".repeat(32), stage: "probation".into(), reasons: vec!["never got through".into()], ..Default::default() };
    let f = compose_feedback("The hub goes blank after the update", Some(failure), 5).unwrap();
    heard_feedback(&store, "Priya", &serde_json::to_string(&f).unwrap()).unwrap();
    std::fs::write(atlas::upgrade::keep_old_at(&root, "0.0.9"), b"old").unwrap();
    let mut d = daemon(&root, &c, &p);
    let _ = d.tray.hand("https://example.com/plan", &atlas::earned::Space::Personal, "the hub", 10);
    for (name, action) in [
        ("updates", Action::Hub(Page::Updates)),
        ("updates-confirm", Action::HubQ(Page::Updates, "confirm=undo".into())),
        ("feedback", Action::Hub(Page::Feedback)),
        ("documents", Action::Hub(Page::Documents)),
        ("partners", Action::Hub(Page::Partners)),
        ("connections", Action::Hub(Page::Connections)),
    ] {
        let html = atlas::hublive::reply(&mut d, action).body;
        std::fs::write(out.join(format!("{name}.html")), html).unwrap();
    }
    let _ = post(&mut d, "/hub/feedback", &[("what", "preview"), ("words", "It crashed")]);
    std::fs::write(out.join("feedback-preview.html"), atlas::hublive::reply(&mut d, Action::Hub(Page::Feedback)).body).unwrap();
}

/// The release key is made on the Updates page, never in a terminal. The
/// making itself is tested in `release` (it writes a vault, which a test
/// here must not do to the shared install); this is the page around it.
#[test]
fn the_release_key_is_made_from_the_updates_page_with_the_recovery_key_shown_once() {
    use atlas::hubpages::{updates_page, ReleaseKey, UpdatesView};
    let base = UpdatesView { version: "0.1.0".into(), mode: "default".into(), ..UpdatesView::default() };
    let page = |key: ReleaseKey| updates_page(&UpdatesView { key, ..base.clone() }, None);
    let first = page(ReleaseKey::NotMade { vault_set: false });
    assert!(first.contains("Make my release key") && first.contains("name=again"), "a first passphrase isn't asked twice");
    assert!(first.contains("type=password autocomplete=new-password"));
    let open = page(ReleaseKey::NotMade { vault_set: true });
    assert!(!open.contains("name=again") && open.contains("autocomplete=current-password"));
    let card = format!("release={}\nrecovery={}", "ab".repeat(32), "cd".repeat(32));
    let just = page(ReleaseKey::JustMade { card: card.clone(), recovery: "11111111 22222222".into() });
    assert!(just.contains("Write this down now") && just.contains("11111111 22222222") && just.contains(&"ab".repeat(32)));
    let later = page(ReleaseKey::Made { card });
    assert!(later.contains("send this card to Claude") && !later.contains("11111111"), "the recovery key is shown again");
    assert!(!page(ReleaseKey::InTheBuild).contains("release key</h2>"), "a build with a key still offers to make one");

    // Through the daemon: a mismatch makes nothing, and a build without a
    // key offers the button.
    let (c, p) = (cfg(), plat());
    let root = install("relkey");
    let mut d = daemon(&root, &c, &p);
    if !atlas::release::anchor_configured() && !d.vault.has_a_passphrase() {
        let html = atlas::hublive::reply(&mut d, Action::Hub(Page::Updates)).body;
        assert!(html.contains("Make my release key"), "the button isn't offered");
        let html = post(&mut d, "/hub/updates", &[("what", "make-key"), ("passphrase", "one"), ("again", "two")]);
        assert!(html.contains("passphrases were different"), "{html}");
        assert!(!d.vault.list().iter().any(|(n, _)| *n == atlas::release::RELEASE_KEY_NAME));
    }
}

/// A Windows program as far as the finder cares: its header, and a version
/// block saying it's Atlas (after the Edge one WebView2 brings).
fn program(version: &str) -> Vec<u8> {
    let block = |pairs: &[(&str, &str)]| {
        let mut b = Vec::new();
        for (k, v) in pairs {
            b.extend([0u8; 6]);
            for s in [k, v] {
                b.extend(s.encode_utf16().chain([0]).flat_map(|u| u.to_le_bytes()));
                while b.len() % 4 != 0 {
                    b.push(0);
                }
            }
        }
        b
    };
    let mut b = vec![0u8; 0x40];
    b[..2].copy_from_slice(b"MZ");
    b[0x3C..0x40].copy_from_slice(&0x40u32.to_le_bytes());
    b.extend(b"PE\0\0");
    b.extend(0x8664u16.to_le_bytes());
    b.extend([0u8; 600]);
    b.extend(block(&[("ProductName", "Microsoft Edge"), ("ProductVersion", "129.0")]));
    b.extend([0u8; 600]);
    b.extend(block(&[("ProductName", "Atlas"), ("ProductVersion", version)]));
    b
}

/// Sending an update needs no terminal either (Eric, 27 Sep 2026): the page
/// finds the build in Downloads, the passphrase signs it, and it's queued for
/// friends. Signed exactly as shown, never twice, and the vault goes back to
/// how it was.
#[test]
fn an_update_is_signed_and_sent_from_the_updates_page() {
    let (c, p) = (cfg(), plat());
    let root = install("send");
    let builds = root.join("Downloads");
    std::fs::create_dir_all(&builds).unwrap();
    let mut d = daemon(&root, &c, &p);
    // Nobody's releaser yet: no section at all.
    let html = atlas::hublive::reply(&mut d, Action::Hub(Page::Updates)).body;
    assert!(!html.contains("<h2 id=su>"), "a friend's Atlas offers to send updates");
    // The releaser: a key in the vault, and a build that trusts it.
    d.vault.open("a long passphrase", 1_000, &atlas::vault::VaultConfig::default()).unwrap();
    atlas::release::make_release_key(&mut d.vault, 1_000).unwrap();
    let seed = atlas::release::seed_from_hex(&d.vault.get(atlas::release::RELEASE_KEY_NAME, 1_000).unwrap()).unwrap();
    let anchor = atlas::release::anchor_of(&atlas::release::signing_key_from_seed(&seed));
    d.vault.lock();
    d.release_setup_for_test(anchor, builds.clone());

    let html = atlas::hublive::reply(&mut d, Action::Hub(Page::Updates)).body;
    assert!(html.contains("<h2 id=su>") && html.contains("No new build of Atlas"), "{html}");
    let prog = program("0.2.0");
    std::fs::write(builds.join("atlas.exe"), &prog).unwrap();
    let html = atlas::hublive::reply(&mut d, Action::Hub(Page::Updates)).body;
    assert!(html.contains("Found <b>atlas.exe</b>") && html.contains("Sign and send Atlas 0.2.0"), "{html}");
    let sha = atlas::digest::sha256_hex(&prog);
    assert!(html.contains(&sha), "the button doesn't say which build it signs");

    let outbox = root.join("data/state").join(atlas::update_courier::OUTBOX);
    let html = post(&mut d, "/hub/updates", &[("what", "sign-send"), ("sha", "0000"), ("passphrase", "a long passphrase")]);
    assert!(html.contains("changed since this page was drawn"), "{html}");
    let html = post(&mut d, "/hub/updates", &[("what", "sign-send"), ("sha", &sha), ("passphrase", "not it")]);
    assert!(!outbox.exists(), "a wrong passphrase sent something: {html}");
    assert!(!html.contains("not it"), "the passphrase came back in the page");

    let html = post(&mut d, "/hub/updates", &[("what", "sign-send"), ("sha", &sha), ("passphrase", "a long passphrase")]);
    assert!(html.contains("Signed Atlas 0.2.0 as release 1.") && html.contains("No group of friends gets updates from you yet"), "{html}");
    assert_eq!(d.vault.state(), atlas::vault::State::Sealed, "the vault was left open");
    let notice = std::fs::read_to_string(outbox.join("atlas-release-0.2.0.json")).unwrap();
    let signed: atlas::release::SignedManifest = serde_json::from_str(&notice).unwrap();
    assert!(signed.manifest.contains(&sha) && signed.manifest.contains("windows-x86_64"));
    assert!(root.join("data/state").join(atlas::update_courier::FILES).join(&sha).exists(), "friends have nothing to fetch");
    // Never twice.
    let html = post(&mut d, "/hub/updates", &[("what", "sign-send"), ("sha", &sha), ("passphrase", "a long passphrase")]);
    assert!(html.contains("already went out") && html.contains("went out as release 1"), "{html}");
    // The next build is the next release.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let next = program("0.2.1");
    std::fs::write(builds.join("Atlas Setup.exe"), &next).unwrap();
    let html = post(&mut d, "/hub/updates", &[("what", "sign-send"), ("sha", &atlas::digest::sha256_hex(&next)), ("passphrase", "a long passphrase")]);
    assert!(html.contains("Signed Atlas 0.2.1 as release 2."), "{html}");
    let _ = std::fs::remove_dir_all(&root);
}
