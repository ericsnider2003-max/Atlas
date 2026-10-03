//! Item 24 (P6): "ask the laptop" from the phone, and hear back. The phone
//! queues the request in your sealed bundles, the laptop runs what only asks
//! to be told something and holds anything that might change something for
//! your yes, and the answer comes back the same way. And Apple Weather for
//! your other devices: a token the laptop carries, never the key.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn root(tag: &str) -> PathBuf {
    let r = std::env::temp_dir().join(format!("atlas-askthelaptop-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&r);
    std::fs::create_dir_all(&r).unwrap();
    r
}

/// A laptop and a phone sharing a household key and a carrier folder.
fn pair(tag: &str) -> (Config, Config, Store, Store, String) {
    let r = root(tag);
    let folder = r.join("carrier");
    std::fs::create_dir_all(&folder).unwrap();
    let phrase = atlas::sync::new_key_phrase();
    let make = |name: &str| {
        let mut c = Config::load(Path::new("config")).unwrap();
        let t = c.tools.as_mut().unwrap();
        t.sync.enabled = true;
        t.sync.name = name.into();
        t.sync.encrypt_bundles = true;
        t.sync.folder = folder.to_string_lossy().into_owned();
        c
    };
    let (ls, ps) = (Store::new(r.join("laptop")), Store::new(r.join("phone")));
    for s in [&ls, &ps] {
        s.save(atlas::sync::KEY_FILE, &atlas::sync::KeptKey::keeping(&phrase, 1_700_000_000)).unwrap();
    }
    (make("laptop"), make("phone"), ls, ps, phrase)
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, s: &Store) -> Daemon<'a> {
    Daemon::new(c, p, None, s.clone(), Proactive::new(ProactiveConfig::default()))
}

const T: u64 = 1_790_900_000;

#[test]
fn the_words_that_hand_something_to_the_laptop() {
    use atlas::remote::handed_over;
    assert_eq!(handed_over("ask the laptop to find the contract").as_deref(), Some("find the contract"));
    assert_eq!(handed_over("On my laptop, check the render finished.").as_deref(), Some("check the render finished"));
    assert_eq!(handed_over("have my laptop look up the Hendricks quote").as_deref(), Some("look up the Hendricks quote"));
    assert_eq!(handed_over("ask the laptop"), None);
    assert_eq!(handed_over("what's on the laptop"), None);
    assert_eq!(handed_over("find the contract"), None);
    assert!(atlas::remote::go_ahead_on_the_laptop("Yes, go ahead on the laptop."));
    assert!(!atlas::remote::go_ahead_on_the_laptop("go ahead"));
}

#[test]
fn said_on_the_laptop_itself_it_is_just_done() {
    let (lc, _, ls, _, _) = pair("self");
    let p = plat();
    let mut l = daemon(&lc, &p, &ls);
    let reply = l.turn("ask the laptop to check what time it is", T);
    assert!(!reply.contains("Sent"), "the laptop sent it to itself: {reply}");
}

#[test]
fn a_reading_runs_on_the_laptop_and_the_answer_comes_back_to_the_phone() {
    let (lc, pc, ls, ps, _) = pair("reading");
    let (lp, pp) = (plat(), plat());
    pp.be_a_phone_for_test();
    let mut phone = daemon(&pc, &pp, &ps);
    let said = phone.turn("ask the laptop to check what time it is", T);
    assert!(said.starts_with("Sent \"check what time it is\" to your laptop"), "{said}");

    let mut laptop = daemon(&lc, &lp, &ls);
    laptop.turn("sync", T + 10);
    let waiting: Vec<serde_json::Value> = ls.load("asked_by_your_phone");
    assert_eq!(waiting.len(), 1, "the laptop never took the phone's request");
    laptop.tick(T + 20);
    let waiting: Vec<serde_json::Value> = ls.load("asked_by_your_phone");
    assert!(waiting.is_empty(), "a reading was held: {waiting:?}");
    laptop.turn("sync", T + 30);

    let back = phone.turn("sync", T + 40);
    assert!(back.contains("From your laptop, on \"check what time it is\""), "the answer never reached the phone: {back}");
    let q: atlas::remote::Queue = ps.load("asked_of_the_laptop");
    assert_eq!(q.requests[0].state, atlas::remote::State::Done);
}

#[test]
fn something_that_might_change_things_waits_for_your_yes_from_the_phone() {
    let (lc, pc, ls, ps, _) = pair("held");
    let (lp, pp) = (plat(), plat());
    pp.be_a_phone_for_test();
    let mut phone = daemon(&pc, &pp, &ps);
    phone.turn("ask the laptop to delete the old installers in downloads", T);
    let mut laptop = daemon(&lc, &lp, &ls);
    laptop.turn("sync", T + 10);
    laptop.tick(T + 20);
    let held: Vec<serde_json::Value> = ls.load("asked_by_your_phone");
    assert_eq!(held.len(), 1);
    assert_eq!(held[0]["held"], true, "a change ran without a yes");
    laptop.turn("sync", T + 30);
    let back = phone.turn("sync", T + 40);
    assert!(back.contains("waiting for your yes") && back.contains("go ahead on the laptop"), "{back}");

    // The yes, from the phone.
    let ok = phone.turn("go ahead on the laptop", T + 50);
    assert!(ok.contains("Told the laptop to go ahead"), "{ok}");
    laptop.turn("sync", T + 60);
    let after: Vec<serde_json::Value> = ls.load("asked_by_your_phone");
    assert!(after.is_empty(), "the yes never reached the held request");
}

#[test]
fn a_request_in_somebody_elses_bundle_is_never_run() {
    // Unsealed: no household key on either side.
    let r = root("unsealed");
    let folder = r.join("carrier");
    std::fs::create_dir_all(&folder).unwrap();
    let make = |name: &str| {
        let mut c = Config::load(Path::new("config")).unwrap();
        let t = c.tools.as_mut().unwrap();
        t.sync.enabled = true;
        t.sync.name = name.into();
        t.sync.encrypt_bundles = false;
        t.sync.folder = folder.to_string_lossy().into_owned();
        c
    };
    let (lc, pc) = (make("laptop"), make("phone"));
    let (ls, ps) = (Store::new(r.join("laptop")), Store::new(r.join("phone")));
    let (lp, pp) = (plat(), plat());
    pp.be_a_phone_for_test();
    let mut phone = daemon(&pc, &pp, &ps);
    phone.turn("ask the laptop to check what time it is", T);
    let mut laptop = daemon(&lc, &lp, &ls);
    laptop.turn("sync", T + 10);
    let waiting: Vec<serde_json::Value> = ls.load("asked_by_your_phone");
    assert!(waiting.is_empty(), "an unsealed bundle's request was taken");
}

#[test]
fn apple_weather_reaches_your_other_devices_as_a_token_and_never_as_the_key() {
    use atlas::applewx::{self, CarriedToken};
    use p256::pkcs8::EncodePrivateKey;
    let r = root("wx");
    let key = p256::SecretKey::from_slice(&[7u8; 32]).unwrap().to_pkcs8_pem(Default::default()).unwrap();
    std::fs::write(r.join("AuthKey_TEST000000.p8"), key.as_bytes()).unwrap();
    let apns = atlas::apns::ApnsConfig {
        key_file: r.join("AuthKey_TEST000000.p8").to_string_lossy().into_owned(),
        key_id: "TEST000000".into(),
        team_id: "Z6NSM9AXB7".into(),
        topic: "com.ericsnider.atlas".into(),
    };
    // The laptop, with the key: a week's token, then nothing until three days are left.
    let ((id, field, to), until) = applewx::carry_token(&apns, &r, "laptop", 0, T).expect("a token to carry");
    assert_eq!(field, "weatherkit");
    assert_eq!(until, T + applewx::CARRIED_LIFE_SECS);
    assert!(!to.contains("PRIVATE KEY"), "the key itself was carried");
    assert!(applewx::carry_token(&apns, &r, "laptop", until, T + 86_400).is_none());
    assert!(applewx::carry_token(&apns, &r, "laptop", until, until - 2 * 86_400).is_some());
    // A friend's Atlas, with no key: nothing to carry.
    assert!(applewx::carry_token(&atlas::apns::ApnsConfig::default(), &r, "friend", 0, T).is_none());

    // The phone: only from your own (sealed) bundle, and then it can ask Apple.
    let state = r.join("phone-state");
    assert_eq!(applewx::take_synced(&state, &id, &to, false, T), None);
    assert!(applewx::take_synced(&state, &id, &to, true, T).is_some());
    let held = CarriedToken::load(&state);
    assert_eq!(held.until, until);
    assert!(held.usable(T).is_some());
    assert!(held.usable(until).is_none(), "an expired token is used");
    // An older one doesn't replace a newer one.
    assert_eq!(applewx::take_synced(&state, &id, &to, true, T + 5), None);
}
