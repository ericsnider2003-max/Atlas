//! Taking things off the Outstanding page (2 Oct 2026).
//!
//! Eric: "Can't remove things from the outstanding list." The page had four
//! lanes and no remove button on any of them, while its own footer said
//! "tell me to drop it and it's gone". Saying so only reached the backlog,
//! one of the four lanes, so anything else on the page "wasn't on the list".
//!
//! Each test presses the button the page actually drew (the key is read out
//! of the rendered HTML, not made up here), follows the redirect back to
//! Outstanding, and then starts a second Atlas on the same store to show it
//! stays gone.

use atlas::backlog::Blocker;
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::hub::Page;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::server::{route, Action, Reply, Request};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}
fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}
fn install(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("atlas-outstanding-drop-{tag}-{}", std::process::id()));
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

fn page(d: &mut Daemon) -> String {
    atlas::hublive::reply(d, Action::Hub(Page::Outstanding)).body
}

/// The key on the remove button drawn in the same item as `what`.
fn key_beside(html: &str, what: &str) -> String {
    let at = html.find(what).unwrap_or_else(|| panic!("{what:?} isn't on the page: {html}"));
    let rest = &html[at..];
    let form = rest.find("action=/hub/outstanding").unwrap_or_else(|| panic!("no remove button after {what:?}"));
    let rest = &rest[form..];
    let v = rest.find("name=key value='").unwrap() + "name=key value='".len();
    rest[v..v + rest[v..].find('\'').unwrap()].to_string()
}

/// Press it the way the browser does: the form body through the server's
/// own router, then the redirect followed back to a page.
fn press(d: &mut Daemon, key: &str) -> String {
    let req = Request {
        method: "POST".into(),
        path: "/hub/outstanding".into(),
        query: String::new(),
        token: None,
        token_from_url: false,
        body: format!("what=drop&key={}", key.replace(':', "%3A")),
    };
    let action = route(&req).expect("the Outstanding button reaches nothing");
    let r: Reply = atlas::hublive::reply(d, action);
    assert_eq!(r.status, 303, "the button should come back to the page: {}", r.body);
    assert!(r.body.starts_with("/hub/outstanding?said="), "{}", r.body);
    let q = r.body.split_once('?').unwrap().1.to_string();
    atlas::hublive::reply(d, Action::HubQ(Page::Outstanding, q)).body
}

fn item(id: &str, title: &str, at: u64) -> atlas::workspace_view::Item {
    atlas::workspace_view::Item {
        id: id.into(),
        title: title.into(),
        kind: atlas::workspace_view::Kind::Task,
        status: atlas::workspace_view::Status::Waiting,
        due: None,
        project: None,
        client: None,
        links: Vec::new(),
        blocked_by: None,
        from: atlas::workspace_view::Origin::YouSaid,
        at,
        closed_at: None,
        tags: Vec::new(),
        estimate_mins: None,
        spent_mins: 0,
        atlas_could: atlas::workspace_view::Handoff::Unknown,
        thinking: Vec::new(),
    }
}

#[test]
fn a_blocked_item_dropped_from_the_page_stays_dropped_after_a_restart() {
    let (c, p) = (cfg(), plat());
    let root = install("blocked");
    let t = atlas::store::now();
    {
        let mut d = daemon(&root, &c, &p);
        d.backlog.record("open chrome and sort my tabs", Blocker::NoScreenGap, t - 3 * 86_400);
        d.backlog.record("research the quarterly budget", Blocker::Offline, t);
        d.backlog.record("send the invoice to Sam", Blocker::NeedsApproval, t);
        d.backlog.save(&d.store).unwrap();

        let html = page(&mut d);
        assert_eq!(html.matches(">Drop it</button>").count(), 3, "every backlog item has its button: {html}");

        let html = press(&mut d, &key_beside(&html, "Open chrome and sort my tabs."));
        assert!(html.contains("class=notice"), "the page says nothing about it");
        assert!(html.contains("is off your outstanding list"), "{html}");
        assert!(!html.contains("<div class=t>Open chrome and sort my tabs.</div>"), "still on the page: {html}");
        assert!(html.contains("Research the quarterly budget."), "took the wrong one off");

        // A waiting-on-you item has the button too.
        let html = press(&mut d, &key_beside(&html, "Send the invoice to Sam."));
        assert!(!html.contains("<div class=t>Send the invoice to Sam.</div>"), "{html}");
    }
    // A second Atlas on the same store: still gone, and still findable
    // with "bring back what I dropped".
    let mut d = daemon(&root, &c, &p);
    let html = page(&mut d);
    assert!(!html.contains("Open chrome"), "came back after a restart: {html}");
    assert!(!html.contains("Send the invoice to Sam"), "came back after a restart");
    assert!(html.contains("Research the quarterly budget."));
    assert_eq!(d.backlog.outstanding().len(), 1);
    assert!(d.dropped.iter().any(|x| x.title == "open chrome and sort my tabs"), "not kept with what you dropped");
}

#[test]
fn pressing_it_twice_takes_off_nothing_else() {
    let (c, p) = (cfg(), plat());
    let root = install("twice");
    let mut d = daemon(&root, &c, &p);
    let t = atlas::store::now();
    d.backlog.record("open chrome and sort my tabs", Blocker::NoScreenGap, t);
    d.backlog.record("research the quarterly budget", Blocker::Offline, t);
    let key = key_beside(&page(&mut d), "Open chrome");
    press(&mut d, &key);
    let html = press(&mut d, &key);
    assert!(html.contains("already off the list"), "{html}");
    assert_eq!(d.backlog.outstanding().len(), 1, "a second press took something else off");
    // And a key that names nothing changes nothing.
    let html = press(&mut d, "z:1");
    assert!(html.contains("already off the list"), "{html}");
    assert_eq!(d.backlog.outstanding().len(), 1);
}

#[test]
fn a_carried_workspace_item_is_marked_dropped_and_kept_that_way() {
    let (c, p) = (cfg(), plat());
    let root = install("carried");
    let t = atlas::store::now();
    {
        let mut d = daemon(&root, &c, &p);
        d.workspace.push(item("receipts", "Sort the receipts", t - 4 * 86_400));
        let html = page(&mut d);
        assert!(html.contains("Sort the receipts"), "{html}");
        let html = press(&mut d, &key_beside(&html, "Sort the receipts"));
        assert!(html.contains("&quot;Sort the receipts&quot; is off your outstanding list"), "{html}");
        assert!(!html.contains("<span class=w>Sort the receipts</span>"), "still carried: {html}");
        let i = d.workspace.iter().find(|i| i.id == "receipts").unwrap();
        assert_eq!(i.status, atlas::workspace_view::Status::Dropped, "deleted rather than marked");
        assert!(i.closed_at.is_some());
    }
    let mut d = daemon(&root, &c, &p);
    assert_eq!(d.workspace.len(), 1, "the workspace wasn't kept");
    assert!(!page(&mut d).contains("Sort the receipts"), "came back after a restart");
}

#[test]
fn a_queued_task_is_cancelled_and_a_running_one_has_no_button() {
    let (c, p) = (cfg(), plat());
    let root = install("queue");
    {
        let mut d = daemon(&root, &c, &p);
        d.queue.push("tidy the downloads folder", atlas::lanes::Lane::Background);
        let running = d.queue.push("back up my notes", atlas::lanes::Lane::Background);
        d.queue.tasks.iter_mut().find(|t| t.id == running).unwrap().state = atlas::lanes::TaskState::Running;
        let html = page(&mut d);
        assert_eq!(html.matches("action=/hub/outstanding").count(), 1, "a running task got a button it can't honour: {html}");
        let html = press(&mut d, &key_beside(&html, "Tidy the downloads folder."));
        assert!(!html.contains("Tidy the downloads folder."), "{html}");
        assert!(html.contains("Back up my notes."), "the running one went too");
        // Asked by key anyway, the running one is said, not silently dropped.
        let html = press(&mut d, &format!("t:{running}"));
        assert!(html.contains("already running"), "{html}");
    }
    let mut d = daemon(&root, &c, &p);
    assert!(!page(&mut d).contains("Tidy the downloads folder."), "came back after a restart");
}

#[test]
fn a_workers_errand_is_asked_to_stop() {
    let (c, p) = (cfg(), plat());
    let root = install("errand");
    let mut d = daemon(&root, &c, &p);
    let work: atlas::crew::Work = Box::new(|ctl: &atlas::crew::Control| {
        while !ctl.checkpoint() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        Err("stopped".into())
    });
    let id = d.crew.hand("research the tide tables", 10, work).unwrap();
    let html = page(&mut d);
    assert_eq!(key_beside(&html, "research the tide tables"), format!("e:{id}"));
    assert!(html.contains(">Stop it</button>"), "{html}");
    let html = press(&mut d, &format!("e:{id}"));
    assert!(html.contains("to stop"), "{html}");
    let started = std::time::Instant::now();
    while d.crew.in_hand(id) && started.elapsed().as_secs() < 10 {
        d.crew.settle(atlas::store::now());
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(!d.crew.in_hand(id), "asked to stop and kept going");
    assert!(!page(&mut d).contains("research the tide tables"));
}

#[test]
fn saying_it_reaches_more_than_the_backlog() {
    let (c, p) = (cfg(), plat());
    let root = install("said");
    let t = atlas::store::now();
    {
        let mut d = daemon(&root, &c, &p);
        d.workspace.push(item("receipts", "Sort the receipts", t - 4 * 86_400));
        d.backlog.record("open chrome and sort my tabs", Blocker::NoScreenGap, t);
        let said = d.turn("take the receipts off my outstanding list", t + 1);
        assert!(said.contains("Sort the receipts") && said.contains("off your outstanding list"), "{said}");
        let said = d.execute(&atlas::intent::Intent::DropTask("drop the task chrome".into()));
        assert!(said.starts_with("Dropped \"open chrome and sort my tabs\""), "{said}");
        let html = page(&mut d);
        assert!(!html.contains("Sort the receipts") && !html.contains("Open chrome"), "{html}");
    }
    let mut d = daemon(&root, &c, &p);
    let html = page(&mut d);
    assert!(!html.contains("Sort the receipts") && !html.contains("Open chrome"), "came back after a restart: {html}");
}
