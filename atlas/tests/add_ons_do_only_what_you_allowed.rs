//! An add-on does only what you allowed, through the real daemon.
//!
//! `plugins.rs` tests the rules one at a time. These say the sentence, the way
//! you would, and check what Atlas actually did -- because the failure this
//! tree keeps finding is a rule that is correct and that nothing calls.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::plugins::{self, Approvals};
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-addons-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(p.join("state")).unwrap();
    std::fs::create_dir_all(p.join("plugins")).unwrap();
    p
}
fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}
fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

const ADDON: &str = "\
plugin_api: 1
id: wind-down
name: Wind down
author: a friend
permissions: [basics, desktop]
flows:
  - name: wind down
    triggers: [\"wind things down\"]
    steps:
      - command: resume
      - command: close chrome
";

struct Rig {
    root: PathBuf,
}

impl Rig {
    fn new(tag: &str, text: &str) -> Rig {
        let root = tmp(tag);
        std::fs::create_dir_all(root.join("plugins/wind-down")).unwrap();
        std::fs::write(root.join("plugins/wind-down/plugin.yaml"), text).unwrap();
        Rig { root }
    }
    fn store(&self) -> Store {
        Store::new(self.root.join("state"))
    }
    fn dir(&self) -> PathBuf {
        self.root.join("plugins")
    }
    fn daemon<'a>(&self, c: &'a Config, p: &'a MockPlatform) -> Daemon<'a> {
        let mut d = Daemon::new(c, p, None, self.store(), Proactive::new(ProactiveConfig::default()));
        d.plugins_dir = self.dir();
        d
    }
    fn approve(&self, c: &Config) {
        let found = plugins::scan(&self.dir(), &c.commands, &Approvals::default());
        plugins::approve(&self.store(), &self.dir(), &c.commands, "wind-down", &found[0].sha256).unwrap();
    }
}

/// Is the add-on's sequence the work Atlas has in hand?
fn running_wind_down(d: &Daemon) -> bool {
    d.mind.focus().is_some_and(|w| w.asked == "wind down" && w.progress().0 < 2)
}

#[test]
fn an_add_on_you_have_not_approved_does_nothing() {
    let (c, p) = (cfg(), plat());
    let rig = Rig::new("unapproved", ADDON);
    let mut d = rig.daemon(&c, &p);
    let reply = d.turn("wind things down", 1_000);
    assert!(!reply.contains("add-on"), "an unapproved add-on ran: {reply}");
    assert!(!running_wind_down(&d), "an unapproved add-on is in flight");
}

#[test]
fn an_approved_add_on_runs_and_still_asks_before_a_consequential_step() {
    let (c, p) = (cfg(), plat());
    let rig = Rig::new("approved", ADDON);
    rig.approve(&c);
    let mut d = rig.daemon(&c, &p);
    let reply = d.turn("wind things down", 1_000);
    assert!(reply.contains("Running wind down (the wind-down add-on), 2 steps"), "{reply}");
    // Closing an app asks, add-on or not, and says whose step it is.
    assert!(reply.contains("from the wind-down add-on") && reply.contains("go ahead?"), "{reply}");
    let w = d.mind.focus().expect("the add-on's sequence is the work in hand");
    assert_eq!(w.asked, "wind down");
    assert_eq!(w.progress(), (1, 2), "the first step ran; the one that asks has not");
}

#[test]
fn a_permission_taken_away_while_it_waits_stops_it_even_after_your_yes() {
    let (c, p) = (cfg(), plat());
    let rig = Rig::new("revoked", ADDON);
    rig.approve(&c);
    let mut d = rig.daemon(&c, &p);
    let asked = d.turn("wind things down", 1_000);
    assert!(asked.contains("go ahead?"), "{asked}");
    plugins::revoke(&rig.store(), "wind-down", "desktop").unwrap();
    let reply = d.turn("yes", 1_010);
    assert!(reply.contains("haven't allowed"), "the revoked step ran anyway: {reply}");
    assert!(!running_wind_down(&d), "a stopped add-on is still in flight");
}

#[test]
fn nothing_you_say_to_answer_a_question_can_start_an_add_on() {
    let (c, p) = (cfg(), plat());
    let rig = Rig::new("pending", ADDON);
    rig.approve(&c);
    let mut d = rig.daemon(&c, &p);
    // One of your own sequences pauses to ask.
    d.flows.record("mine", &["resume".into(), "close notepad".into()], Some("my own wind down"));
    let asked = d.turn("my own wind down", 1_000);
    assert!(asked.contains("go ahead?"), "{asked}");
    let reply = d.turn("wind things down", 1_010);
    assert!(!reply.contains("add-on"), "an add-on started while Atlas was waiting on an answer: {reply}");
    assert!(!running_wind_down(&d), "the add-on is in flight");
}

#[test]
fn a_file_changed_after_approval_does_not_run() {
    let (c, p) = (cfg(), plat());
    let rig = Rig::new("swapped", ADDON);
    rig.approve(&c);
    std::fs::write(
        rig.dir().join("wind-down/plugin.yaml"),
        ADDON.replace("close chrome", "close notepad"),
    )
    .unwrap();
    let mut d = rig.daemon(&c, &p);
    let reply = d.turn("wind things down", 1_000);
    assert!(!reply.contains("add-on"), "a changed add-on ran: {reply}");
    assert!(!running_wind_down(&d), "a changed add-on is in flight");
}

#[test]
fn your_own_sequence_wins_over_an_add_on_with_the_same_words() {
    let (c, p) = (cfg(), plat());
    let rig = Rig::new("yours-first", ADDON);
    rig.approve(&c);
    let mut d = rig.daemon(&c, &p);
    d.flows.record("mine", &["resume".into()], Some("wind things down"));
    let reply = d.turn("wind things down", 1_000);
    assert!(reply.contains("Running mine"), "{reply}");
    assert!(!reply.contains("add-on"), "{reply}");
    assert!(!running_wind_down(&d), "the add-on ran instead of yours");
}

// ---------------------------------------------------------------- the hub

#[test]
fn the_add_ons_page_is_reachable_and_its_buttons_post_somewhere_real() {
    use atlas::hub::{self, Page};
    assert_eq!(hub::route("/hub/addons"), Some(Page::AddOns));
    assert_eq!(hub::route("/hub/edits"), Some(Page::Edits));
    assert!(hub::works_without_voice(Page::AddOns) && hub::works_without_voice(Page::Edits));

    let c = cfg();
    let rig = Rig::new("hubpage", ADDON);
    let found = plugins::scan(&rig.dir(), &c.commands, &Approvals::default());
    let html = hub::addons_page_with(&found, &[], &[], &[]);
    assert!(html.contains("action=/hub/addons"), "no button posts to the add-ons route");
    assert!(html.contains(&found[0].sha256), "approve must carry the fingerprint of what was shown");
    assert!(html.contains("open, close and arrange apps"), "permissions must be shown in words");

    // The route the button posts to turns into the action that approves.
    let req = atlas::server::Request {
        method: "POST".into(),
        path: "/hub/addons".into(),
        query: String::new(),
        token: None,
        token_from_url: false,
        body: format!("what=approve&id=wind-down&sha={}", found[0].sha256),
    };
    match atlas::server::route(&req) {
        Some(atlas::server::Action::AddOn { what, id, sha, .. }) => {
            assert_eq!((what.as_str(), id.as_str()), ("approve", "wind-down"));
            let trash = atlas::safety::Trash::new(atlas::safety::TrashConfig {
                dir: rig.root.join("trash").to_string_lossy().into_owned(),
                keep_days: 30,
            });
            plugins::hub_action(&rig.store(), &rig.dir(), &c.commands, &trash, &what, &id, "", &sha).unwrap();
        }
        other => panic!("POST /hub/addons routed to {other:?}"),
    }
    let now = plugins::scan(&rig.dir(), &c.commands, &Approvals::load(&rig.store()));
    assert_eq!(now[0].status, plugins::Status::Active);
    assert!(hub::addons_page_with(&now, &[], &[], &[]).contains("take this away"));
}

#[test]
fn the_edits_page_offers_the_way_back_for_each_edit() {
    use atlas::yourchanges::{Change, KeptEdit};
    assert!(atlas::hub::edits_page(&[], &[]).contains("No edits of yours"));
    let e = KeptEdit {
        file: "policy.yaml".into(),
        change: Change {
            path: vec!["min_samples".into()],
            yours: Some(serde_yaml::from_str("9").unwrap()),
            removed: false,
            was: Some(serde_yaml::from_str("5").unwrap()),
            unsure: true,
        },
        shipped_now: Some(serde_yaml::from_str("7").unwrap()),
    };
    let html = atlas::hub::edits_page(&[e], &[]);
    assert!(html.contains("action=/hub/edits") && html.contains("Back to the default"));
    assert!(html.contains("value='min_samples'") && html.contains("value='policy.yaml'"));
    assert!(html.contains("couldn't tell"), "an unsure edit must say so");
    assert!(html.contains("The default has changed"), "a moved default must say so");
}


// ---------------------------------------------------------------- asking less

#[test]
fn say_always_once_and_that_step_stops_asking() {
    let (c, p) = (cfg(), plat());
    let rig = Rig::new("always", ADDON);
    rig.approve(&c);
    let mut d = rig.daemon(&c, &p);
    let asked = d.turn("wind things down", 1_000);
    assert!(asked.contains("Say \"always\""), "the question should offer not asking again: {asked}");
    let reply = d.turn("always", 1_010);
    assert!(reply.contains("won't ask"), "{reply}");
    assert!(!running_wind_down(&d), "the approved step should have run");

    // The next run goes straight through.
    let again = d.turn("wind things down", 2_000);
    assert!(!again.contains("go ahead?"), "asked again after you said always: {again}");
    assert!(d.mind.focus().map_or(true, |w| w.asked != "wind down" || w.progress() == (2, 2)));
}

#[test]
fn a_benign_add_on_never_asks_at_all() {
    let (c, p) = (cfg(), plat());
    let rig = Rig::new("benign", &ADDON.replace("close chrome", "what can you do").replace("[basics, desktop]", "[basics]"));
    rig.approve(&c);
    let mut d = rig.daemon(&c, &p);
    let reply = d.turn("wind things down", 1_000);
    assert!(!reply.contains("go ahead?"), "{reply}");
    assert!(!running_wind_down(&d));
}

// ---------------------------------------------------------------- by itself

#[test]
fn a_scheduled_add_on_runs_itself_through_the_same_checks() {
    let (c, p) = (cfg(), plat());
    let text = ADDON
        .replace("    triggers: [\"wind things down\"]\n", "    schedule: every 15 minutes\n")
        .replace("close chrome", "what can you do")
        .replace("[basics, desktop]", "[basics]");
    let rig = Rig::new("scheduled", &text);
    rig.approve(&c);
    let mut d = rig.daemon(&c, &p);
    let first = d.tick(100_000).join(" ");
    assert!(!first.contains("add-on"), "fired on first sight: {first}");
    let later = d.tick(100_000 + 15 * 60).join(" ");
    assert!(later.contains("Running wind down (the wind-down add-on)"), "{later}");
    let soon = d.tick(100_000 + 16 * 60).join(" ");
    assert!(!soon.contains("Running wind down"), "ran twice in one period: {soon}");

    // Switched off: it doesn't run itself either.
    plugins::set_off(&rig.store(), "wind-down", true).unwrap();
    let off = d.tick(100_000 + 60 * 60).join(" ");
    assert!(!off.contains("Running wind down"), "{off}");
}

// ---------------------------------------------------------------- from a friend

#[test]
fn an_add_on_a_friend_shares_waits_for_your_choice_then_runs_as_theirs() {
    let (c, p) = (cfg(), plat());
    let rig = Rig::new("handed", ADDON);
    std::fs::remove_dir_all(rig.dir().join("wind-down")).unwrap();
    let mut d = rig.daemon(&c, &p);
    let said = d.receive_handoff(&atlas::kin::Delivered {
        from: "Sam".into(),
        what: format!("{}Friends", plugins::SHARED_IN),
        at: 1_000,
        file: Some(atlas::kin::DeliveredFile { name: "wind-down.atlas-addon.yaml".into(), bytes: ADDON.as_bytes().to_vec() }),
    });
    assert!(said.contains("Sam shared the add-on") && said.contains("Friends"), "{said}");
    assert!(plugins::scan(&rig.dir(), &c.commands, &Approvals::load(&rig.store())).is_empty(), "installed on arrival");
    let reply = d.turn("wind things down", 2_000);
    assert!(!reply.contains("add-on"), "a shared add-on ran before you chose it: {reply}");
    // Whatever Atlas made of that, let it go before trying again.
    d.turn("never mind", 2_100);

    // You choose it on the Add-ons page.
    let offers = plugins::Offers::load(&rig.store()).items;
    let shelf = atlas::hub::addons_page_with(&[], &offers, &[], &[]);
    assert!(shelf.contains("Add it and allow that"));
    assert!(shelf.contains("close chrome"), "the shelf should show what it would actually do");
    plugins::take_offer(&rig.store(), &rig.dir(), &c.commands, offers[0].offer, &offers[0].sha256).unwrap();
    let found = plugins::scan(&rig.dir(), &c.commands, &Approvals::load(&rig.store()));
    assert_eq!(found[0].status, plugins::Status::Active);
    assert_eq!(found[0].sent_by.as_deref(), Some("Sam"));
    let reply = d.turn("wind things down", 3_000);
    assert!(reply.contains("the wind-down add-on"), "{reply}");
}

// ---------------------------------------------------------------- your other devices

#[test]
fn a_note_and_an_add_on_actually_arrive_on_your_other_device() {
    let folder = tmp("sync-folder");
    let make = |name: &str| {
        let mut c = cfg();
        let t = c.tools.as_mut().unwrap();
        t.sync.enabled = true;
        t.sync.name = name.into();
        t.sync.folder = folder.to_string_lossy().into_owned();
        t.sync.encrypt_bundles = false;
        c
    };
    let (lc, pc, p) = (make("laptop"), make("phone"), plat());
    let laptop = Rig::new("sync-laptop", ADDON);
    laptop.approve(&lc);
    let phone = Rig::new("sync-phone", ADDON);
    std::fs::remove_dir_all(phone.dir().join("wind-down")).unwrap();

    let mut l = laptop.daemon(&lc, &p);
    l.turn("note that the rack needs a ten inch shelf", 1_000);
    l.turn("sync", 1_010);

    let mut ph = phone.daemon(&pc, &p);
    ph.turn("sync", 1_020);
    assert!(
        ph.notebook.notes.iter().any(|n| n.text.contains("ten inch shelf")),
        "the note crossed the folder and never reached the phone's notebook"
    );
    let found = plugins::scan(&phone.dir(), &pc.commands, &Approvals::load(&phone.store()));
    assert_eq!(found.len(), 1, "the add-on didn't follow you");
    assert_eq!(found[0].status, plugins::Status::Waiting, "an approval came through an unsealed bundle");
}
