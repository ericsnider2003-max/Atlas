//! Friends put Atlas on their computer first, set it up, and then get their
//! phone by picking what it is and scanning a code (Eric's ruling on D5, 27
//! Sep 2026). No files, no command prompt: the hub's Your phone page does it.
//! The protocol pieces (the profile, the phone's reply, the real socket) are
//! tested in `phoneadd`; this is the page and what happens to a phone that
//! told Atlas its ID.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::hub::{self, Page};
use atlas::phoneadd::{Device, Kind};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::server::Action;
use atlas::store::Store;
use std::path::{Path, PathBuf};

const UDID: &str = "00008030-001A2B3C4D5E6F70";

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}
fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}
fn install(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("atlas-phoneadd-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("data/state")).unwrap();
    std::fs::create_dir_all(root.join("peers")).unwrap();
    root
}
fn daemon<'a>(root: &Path, c: &'a Config, p: &'a MockPlatform) -> Daemon<'a> {
    let mut d = Daemon::new(c, p, None, Store::new(root.join("data/state")), Proactive::new(ProactiveConfig::default()));
    d.peer_dir = root.join("peers");
    std::fs::create_dir_all(root.join("Downloads")).unwrap();
    d.downloads_for_test(root.join("Downloads"));
    d
}
fn page(d: &mut Daemon, q: &str) -> String {
    atlas::hublive::reply(d, Action::HubQ(Page::Phone, q.into())).body
}

#[test]
fn the_page_is_in_your_devices_and_findable() {
    assert!(hub::NAV.iter().any(|(g, ps)| *g == "Your devices" && ps.contains(&Page::Phone)));
    assert_eq!(hub::route("/hub/phone"), Some(Page::Phone));
    assert!(Page::Phone.reads_query(), "the phone picked would be lost");
    assert!(atlas::palette::catalogue().iter().any(|e| e.label.contains("phone") && matches!(e.does, atlas::palette::Does::Go("/hub/phone"))));
}

#[test]
fn you_pick_the_phone_first_and_each_gets_its_own_steps() {
    let (c, p) = (cfg(), plat());
    let root = install("pick");
    let mut d = daemon(&root, &c, &p);
    let first = page(&mut d, "");
    assert!(first.contains("href='/hub/phone?kind=iphone'") && first.contains("href='/hub/phone?kind=android'"), "{first}");
    assert!(!first.contains("Show the code"), "steps shown before a phone was picked");
    let apple = page(&mut d, "kind=iphone");
    assert!(apple.contains("1. Tell Atlas about it") && apple.contains("value=add") && apple.contains("Show the code"), "{apple}");
    assert!(apple.contains("aria-current='page'"));
    let android = page(&mut d, "kind=android");
    assert!(android.contains("isn't on this computer yet") && android.contains("Downloads"), "{android}");
    // Saved to Downloads, whatever the browser called it: the page takes it in
    // and offers its code. Nobody moves files by hand.
    std::fs::write(root.join("Downloads/Atlas-Android-12.apk"), vec![0u8; 2_500_000]).unwrap();
    std::fs::write(root.join("Downloads/holiday.apk"), vec![0u8; 10]).unwrap();
    let android = page(&mut d, "kind=android");
    assert!(android.contains("The Android app is here (2 MB)") && android.contains("Show the install code"), "{android}");
    assert!(root.join("apps/Atlas.apk").is_file(), "it wasn't kept with the install");
    // A newer download replaces the kept one; an older one doesn't.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(root.join("Downloads/Atlas (1).apk"), vec![0u8; 3_500_000]).unwrap();
    assert!(page(&mut d, "kind=android").contains("The Android app is here (3 MB)"));
}

#[test]
fn without_tailscale_the_page_says_what_to_do_instead_of_a_dead_code() {
    let (c, p) = (cfg(), plat());
    let root = install("notail");
    let mut d = daemon(&root, &c, &p);
    let fields = [("what", "start"), ("kind", "iphone"), ("for", "add")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let r = atlas::hublive::reply(&mut d, Action::HubPost { path: "/hub/phone".into(), fields });
    assert_eq!(r.status, 303);
    // 27 Sep 2026: the code is got ready on the crew (Tailscale can take
    // twenty seconds a call, and the whole of Atlas waited on it), so the
    // page follows a job, then shows what came of it.
    assert!(r.body.starts_with("/hub/phone?kind=iphone&job="), "{}", r.body);
    let (_, q) = r.body.split_once('?').unwrap();
    d.errands_done_for_test();
    let html = page(&mut d, q);
    if which_tailscale().is_none() {
        assert!(html.contains("Tailscale"), "the page doesn't say what's missing: {html}");
        assert!(!html.contains("class=qr"), "a code shown with nothing behind it");
    }
}

fn which_tailscale() -> Option<()> {
    std::process::Command::new("tailscale").arg("version").output().ok().filter(|o| o.status.success()).map(|_| ())
}

#[test]
fn an_iphone_that_told_atlas_its_id_is_kept_and_listed_for_the_next_build() {
    let (c, p) = (cfg(), plat());
    let root = install("heard");
    let mut d = daemon(&root, &c, &p);
    // What the code's server hands over when the phone replies.
    d.phones_heard_for_test(Device { name: "Eric's iPhone".into(), product: "iPhone15,2".into(), udid: UDID.into(), ..Device::default() });
    let html = page(&mut d, "kind=iphone");
    assert!(html.contains("Added") && html.contains("Eric&#39;s iPhone"), "{html}");
    assert!(html.contains("Add another"), "{html}");
    // Not in anyone's update group yet, so it couldn't be sent on: never
    // dropped without a word. The page says so and gives the line to send.
    assert!(html.contains("Not sent on yet") && html.contains(&format!("{UDID}\tEric&#39;s iPhone")) && html.contains("Copy"), "{html}");
    let store = Store::new(root.join("data/state"));
    let mine: Vec<Device> = store.load(atlas::phoneadd::MINE);
    assert_eq!(mine.len(), 1);
    assert!(!mine[0].sent, "marked sent when it wasn't");
    // Looked at again, it's tried again, and still shown until it goes.
    assert!(page(&mut d, "kind=iphone").contains("Not sent on yet"));
    // Heard again: still one.
    d.phones_heard_for_test(mine[0].clone());
    let _ = page(&mut d, "kind=iphone");
    assert_eq!(store.load::<Vec<Device>>(atlas::phoneadd::MINE).len(), 1);
}

#[test]
fn on_the_releasers_atlas_friends_phones_wait_with_a_list_to_send() {
    let (c, p) = (cfg(), plat());
    let root = install("waiting");
    let store = Store::new(root.join("data/state"));
    let d = Device { name: "Sam's iPad".into(), product: "iPad13,4".into(), udid: UDID.into(), ..Device::default() };
    let wire = format!("{}{}", atlas::phoneadd::WIRE_PREFIX, serde_json::to_string(&d).unwrap());
    let said = atlas::phoneadd::heard(&store, "Sam", &wire, 10).unwrap();
    assert!(said.contains("Sam's device") && said.contains("next iPhone build"), "{said}");
    // Arriving as feedback, it goes to the list, not the inbox.
    let f = atlas::feedback::compose_feedback(&wire, None, 11).unwrap();
    assert!(atlas::feedback::heard_feedback(&store, "Sam", &serde_json::to_string(&f).unwrap()).is_some());
    assert!(atlas::feedback::feedback_inbox(&store).is_empty(), "a phone landed in the feedback inbox");
    let mut dd = daemon(&root, &c, &p);
    let html = page(&mut dd, "");
    assert!(html.contains("Waiting for the next iPhone build") && html.contains(UDID) && html.contains("from Sam"), "{html}");
    // Something that isn't a UDID never reaches the list a build is made from.
    let bad = format!("{}{}", atlas::phoneadd::WIRE_PREFIX, serde_json::to_string(&Device { udid: "../../etc".into(), ..d }).unwrap());
    assert!(atlas::phoneadd::heard(&store, "Mallory", &bad, 12).is_none());
    let _ = Kind::Apple;
}
