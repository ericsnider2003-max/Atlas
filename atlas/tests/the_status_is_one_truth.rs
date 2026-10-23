//! Every surface tells the same story about the work (22 Oct 2026).
//!
//! Eric: "Nothing ever showed as in work, failed so how can I trust that it's
//! actually working?" On a fresh Atlas, at the same moment, Home said
//! "Working" on a self-repair run, Now said "Nothing underway right now" and
//! Outstanding said "In progress 2" -- while the run was not running at all:
//! it was waiting for memory. Each page read its own source.
//!
//! Now there is one record (the lanes of `hub::Open`) and every surface draws
//! from it: Home, Now, Outstanding, the header pill and the phone's
//! `/hub/live.json`. These tests build real states and read every surface, so
//! any two that disagree fail the build.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::hub::Page;
use atlas::lanes::{Lane, TaskState};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::server::Action;
use atlas::store::Store;
use std::path::{Path, PathBuf};

const QUEUED_RUN: &str = "Self-repair: mutation sweep of src/accounts.rs";
const QUEUED_WHY: &str =
    "Waiting for you to be away (only 265 MB of memory free, 6056 MB needed) before it starts.";
const RUNNING: &str = "Back up the photos folder.";
const FAILED: &str = "research job";

fn home(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-one-truth-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }])
}

/// A self-repair run waiting for memory, written the way `heavywork` writes
/// it: started, then checkpointed with the reason it is waiting.
fn queued_self_repair(store: &Store) {
    let mut r =
        atlas::job::Record::planned(format!("{}1:0", atlas::heavywork::JOB_PREFIX), QUEUED_RUN);
    r.transition(atlas::job::State::Running, "started").unwrap();
    r.transition(atlas::job::State::Checkpointed, QUEUED_WHY)
        .unwrap();
    r.heartbeat(QUEUED_WHY);
    atlas::job::upsert(store, r).unwrap();
}

fn failed_research(store: &Store) {
    let mut r = atlas::job::Record::planned("research:duplicates", "research");
    r.transition(atlas::job::State::Running, "started").unwrap();
    atlas::job::upsert(store, r).unwrap();
    atlas::job::end(
        store,
        "research:duplicates",
        "research",
        atlas::job::State::Failed,
        "The search service did not answer after 3 tries.",
    )
    .unwrap();
}

/// Something on today's calendar, so Home draws the day's NOW divider.
fn something_today(d: &mut Daemon) {
    let now = atlas::store::now();
    d.calendar.add(
        "Dentist",
        atlas::calendar::When {
            start: now.saturating_sub(3600),
            end: now.saturating_sub(1800),
            all_day: false,
        },
        None,
        now,
    );
}

fn page(d: &mut Daemon, p: Page) -> String {
    atlas::hublive::reply(d, Action::Hub(p)).body
}

fn live(d: &mut Daemon) -> serde_json::Value {
    serde_json::from_str(&atlas::hublive::reply(d, Action::LiveJson).body).unwrap()
}

/// The HTML of one Outstanding lane, by its id.
fn lane<'a>(html: &'a str, id: &str) -> &'a str {
    let start = html
        .find(&format!("id={id}>"))
        .unwrap_or_else(|| panic!("no lane {id}"));
    let rest = &html[start..];
    &rest[..rest.find("</section>").unwrap_or(rest.len())]
}

fn items(lane_html: &str) -> usize {
    lane_html.matches("class=item>").count()
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\'', "&#39;")
}

#[test]
fn queued_work_is_queued_on_every_surface_and_never_working() {
    let root = home("queued");
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = Daemon::new(
        &c,
        &p,
        None,
        Store::new(&root),
        Proactive::new(ProactiveConfig::default()),
    );
    // Written while Atlas runs, as `heavywork` does: a record from before a
    // start is reconciled as cut off, which is a different (true) story.
    queued_self_repair(&d.store);
    something_today(&mut d);

    let j = live(&mut d);
    let counts = &j["work"]["counts"];
    assert_eq!(
        counts["working"], 0,
        "live.json calls queued work running: {j}"
    );
    assert_eq!(counts["queued"], 1, "live.json lost the queued run: {j}");
    assert_eq!(j["work"]["queued"][0]["what"], QUEUED_RUN);
    assert!(
        j["work"]["queued"][0]["why"]
            .as_str()
            .unwrap()
            .contains("6056 MB needed"),
        "the reason it waits must travel with it: {j}"
    );

    let outstanding = page(&mut d, Page::Outstanding);
    assert_eq!(
        items(lane(&outstanding, "lane-running")),
        0,
        "Outstanding puts queued work in progress"
    );
    let q = lane(&outstanding, "lane-queued");
    assert_eq!(items(q), 1, "{q}");
    assert!(q.contains(&esc(QUEUED_RUN)), "{q}");

    let homepage = page(&mut d, Page::Dashboard);
    assert!(
        !homepage.contains(">Working<"),
        "Home shows a Working pill while nothing runs"
    );
    assert!(
        homepage.contains("Queued: Self-repair"),
        "Home must name the queued run under Right now"
    );

    let now = page(&mut d, Page::Now);
    assert!(
        !now.contains("Nothing underway right now."),
        "Now says nothing is underway while a run is queued"
    );
    assert!(
        now.contains("Nothing running right now."),
        "Now must say plainly that nothing runs"
    );
    assert!(
        now.contains(&esc(QUEUED_RUN)),
        "Now must name what is queued"
    );
}

#[test]
fn running_and_failed_work_agree_on_every_surface() {
    let root = home("running");
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = Daemon::new(
        &c,
        &p,
        None,
        Store::new(&root),
        Proactive::new(ProactiveConfig::default()),
    );
    queued_self_repair(&d.store);
    failed_research(&d.store);
    something_today(&mut d);
    let id = d.queue.push("back up the photos folder", Lane::Background);
    d.queue.tasks.iter_mut().find(|t| t.id == id).unwrap().state = TaskState::Running;

    let j = live(&mut d);
    let counts = &j["work"]["counts"];
    let working = counts["working"].as_u64().unwrap() as usize;
    let queued = counts["queued"].as_u64().unwrap() as usize;
    let failed = counts["failed"].as_u64().unwrap() as usize;
    assert_eq!((working, queued, failed), (1, 1, 1), "{j}");

    let outstanding = page(&mut d, Page::Outstanding);
    let running_lane = lane(&outstanding, "lane-running");
    assert_eq!(
        items(running_lane),
        working,
        "Outstanding and live.json disagree on running"
    );
    assert!(running_lane.contains(RUNNING), "{running_lane}");
    assert!(
        !running_lane.contains(&esc(QUEUED_RUN)),
        "queued work leaked into In progress"
    );
    assert_eq!(
        items(lane(&outstanding, "lane-queued")),
        queued,
        "Outstanding and live.json disagree on queued"
    );
    assert!(
        outstanding.contains(FAILED),
        "Outstanding must show the failure"
    );
    assert!(
        outstanding.contains("The search service did not answer after 3 tries."),
        "a failure says why"
    );

    let homepage = page(&mut d, Page::Dashboard);
    assert!(homepage.contains(RUNNING), "Home must name what is running");
    assert!(
        homepage.contains(">Working<"),
        "Home marks running work as Working"
    );
    assert!(
        !homepage.contains("Nothing underway."),
        "Home says nothing is underway while work runs"
    );
    assert!(
        homepage.contains("1 failed or cut off"),
        "the header pill must count the failure the record holds"
    );

    let now = page(&mut d, Page::Now);
    assert!(
        !now.contains("Nothing underway right now."),
        "Now disagrees with Home and Outstanding"
    );
    assert!(
        now.contains(RUNNING),
        "Now must show what Home says is running"
    );
}

/// A self-repair run that is actually running.
fn running_self_repair(store: &Store) {
    let mut r = atlas::job::Record::planned(
        format!("{}2:0", atlas::heavywork::JOB_PREFIX),
        "Self-repair: test sweep",
    );
    r.transition(atlas::job::State::Running, "Running while you're away.")
        .unwrap();
    r.heartbeat("Running while you're away.");
    atlas::job::upsert(store, r).unwrap();
}

#[test]
fn home_names_what_runs_before_what_waits_and_marks_it_working() {
    // The review, 23 Oct 2026: with an errand running and a task queued,
    // Right now said "About to start" under the Working mark.
    let root = home("order");
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = Daemon::new(
        &c,
        &p,
        None,
        Store::new(&root),
        Proactive::new(ProactiveConfig::default()),
    );
    running_self_repair(&d.store);
    d.queue.push("tidy the downloads folder", Lane::Background);
    let deck = d.deck(atlas::store::now(), atlas::localclock::offset_secs());
    assert_eq!(deck.now, "Self-repair: test sweep", "{deck:?}");
    assert!(
        deck.spine
            .iter()
            .any(|(_, _, m)| *m == atlas::hub::Mark::Now),
        "something runs, so the day is marked Working: {:?}",
        deck.spine
    );
}

#[test]
fn a_businesss_tabs_keep_the_business() {
    let root = home("business-tabs");
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = Daemon::new(
        &c,
        &p,
        None,
        Store::new(&root),
        Proactive::new(ProactiveConfig::default()),
    );
    let html = atlas::hublive::reply(&mut d, Action::HubQ(Page::Clients, "b=Acme".into())).body;
    let tabs = &html[html.find("<nav class=subnav").expect("the section's tabs")..];
    let tabs = &tabs[..tabs.find("</nav>").unwrap()];
    for href in [
        "/hub/business?b=Acme",
        "/hub/tasks?b=Acme",
        "/hub/partners?b=Acme",
    ] {
        assert!(tabs.contains(href), "{href} missing from {tabs}");
    }
}
