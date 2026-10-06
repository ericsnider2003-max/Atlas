//! Connecting an account is one button, and never a passphrase (5 Oct 2026).
//!
//! Eric: Y2 and Y3 were "overly complicated and highly annoying for a user",
//! and the vault's passphrase was one nobody would remember. How professional
//! apps link accounts (Buffer, the desktop OAuth guides, Microsoft's own
//! advice on keeping secrets on Windows): one Connect button per service,
//! the service's own sign-in page, and what's kept is sealed to your Windows
//! sign-in -- no key to paste, no developer app to make, no extra password.
//!
//! Off Windows the sign-in seal is a test-only stand-in
//! (`ATLAS_TEST_LOGINSEAL=1`, debug builds only); on Windows it is DPAPI.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use atlas::vault::{Kind, Vault, VaultConfig};
use std::path::Path;

fn stand_in() {
    // The same value from every test in this file, so the order they run in
    // can't matter.
    std::env::set_var("ATLAS_TEST_LOGINSEAL", "1");
}

fn tmp(tag: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-onebutton-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cheap() -> VaultConfig {
    VaultConfig { enabled: true, ..Default::default() }
}

#[test]
fn a_new_vault_opens_on_your_sign_in_with_no_passphrase() {
    stand_in();
    let mut v = Vault::default();
    assert!(v.is_brand_new());
    v.start_on_this_login(100).expect("started");
    assert!(v.opens_on_login_only() && v.sealed_to_this_login());
    assert!(!v.has_a_passphrase(), "nothing was chosen, so nothing is asked for later");
    assert!(!v.proved_it(), "a sign-in is not proof of who's typing");
    v.put("google", Kind::Login, "refresh-token", 100).unwrap();
    v.lock();
    v.open_unattended(200).expect("opens on the sign-in alone");
    assert_eq!(v.get("google", 200).unwrap(), "refresh-token");
    // Started once; never again over what's there.
    assert!(v.start_on_this_login(300).is_err());
}

#[test]
fn a_passphrase_added_later_wraps_the_same_key() {
    stand_in();
    let cfg = cheap();
    let mut v = Vault::default();
    v.start_on_this_login(100).unwrap();
    v.put("youtube", Kind::ApiKey, "k-123", 100).unwrap();
    v.lock();
    // The brand-new branch would make a second key and strand the first.
    v.open("a sentence nobody else would guess, ever", 200, &cfg).expect("adds a passphrase");
    assert!(v.has_a_passphrase());
    assert_eq!(v.get("youtube", 200).unwrap(), "k-123", "what was kept is still readable");
    v.lock();
    v.open("a sentence nobody else would guess, ever", 300, &cfg).expect("and opens with it after");
    assert!(v.proved_it());
    v.lock();
    v.open_unattended(400).expect("and still on the sign-in");
    assert_eq!(v.get("youtube", 400).unwrap(), "k-123");
}

#[test]
fn what_must_never_open_on_a_sign_in_alone_isnt_kept_without_a_passphrase() {
    stand_in();
    let mut v = Vault::default();
    v.start_on_this_login(100).unwrap();
    let e = v.put("bank", Kind::RecoveryCodes, "1234-5678", 100).unwrap_err();
    assert!(e.contains("passphrase"), "{e}");
    assert!(v.put("bsky", Kind::ApiKey, "app-pw", 100).is_ok());
}

fn daemon(tag: &str) -> (std::path::PathBuf, MockPlatform) {
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    (tmp(tag), p)
}

#[test]
fn keeping_a_key_never_asks_for_a_passphrase() {
    stand_in();
    let (dir, p) = daemon("key");
    let c = Config::load(Path::new("config")).unwrap();
    let mut d = Daemon::new(&c, &p, None, Store::new(dir.clone()), Proactive::new(ProactiveConfig::default()));
    let r = atlas::hublive::reply(
        &mut d,
        atlas::server::Action::HubPost {
            path: "/hub/social".into(),
            fields: vec![("what".into(), "key".into()), ("name".into(), "youtube".into()), ("secret".into(), "AIza-test".into())],
        },
    );
    let back = atlas::hub::urldecode(&r.body);
    assert!(back.contains("Kept your YouTube API key"), "{back}");
    assert!(!back.to_lowercase().contains("passphrase") && !back.contains("locked"), "{back}");
    // Really kept, on disk, in a vault that opens on the sign-in.
    let mut v = Vault::load(&Store::new(dir.clone()));
    assert!(v.opens_on_login_only());
    v.open_unattended(1).unwrap();
    assert_eq!(v.get(atlas::social::VAULT_YOUTUBE_KEY, 1).unwrap(), "AIza-test");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_social_page_leads_with_one_connect_list_and_hides_the_paste_boxes() {
    stand_in();
    let (dir, p) = daemon("page");
    let c = Config::load(Path::new("config")).unwrap();
    let mut d = Daemon::new(&c, &p, None, Store::new(dir.clone()), Proactive::new(ProactiveConfig::default()));
    let html = atlas::hublive::reply(&mut d, atlas::server::Action::Hub(atlas::hub::Page::Social)).body;
    let connect = html.find("Connect your accounts").expect("the Connect list");
    let developers = html.find("For developers").expect("the paste boxes, folded away");
    assert!(connect < developers);
    let before = &html[..developers];
    assert!(!before.contains("type=password"), "nothing secret is asked for before the developers' corner");
    assert!(!before.contains("client ID") && !before.contains("client key"), "no developer app is asked for");
    assert!(before.contains("Connect YouTube") || before.contains("built without its Google sign-in"));
    assert!(before.contains("Connect Bluesky"));
    assert!(before.contains("Sign in to your accounts"));
    assert_eq!(before.matches("value='browser-signin-all'").count(), 1, "one button opens every site");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_old_vault_asks_once_and_can_be_started_afresh() {
    stand_in();
    let v = atlas::hub::VaultView { needs_its_passphrase_once: true, has_passphrase: true, nonce: "n".into(), ..Default::default() };
    let html = atlas::hub::vault_section(&v);
    assert!(html.contains("Unlock it this once") && html.contains("Start a new vault"), "{html}");
    let v = atlas::hub::VaultView { opens_on_login: true, nonce: "n".into(), ..Default::default() };
    let html = atlas::hub::vault_section(&v);
    assert!(html.contains("nothing to type"), "{html}");
    let shown = &html[..html.find("<details").expect("the passphrase is folded away")];
    assert!(!shown.contains("type=password"), "no passphrase box in sight: {shown}");
}

#[test]
fn starting_afresh_sets_the_old_vault_aside_and_opens_on_the_sign_in() {
    stand_in();
    let (dir, p) = daemon("fresh");
    let store = Store::new(dir.clone());
    // A vault made the old way: a passphrase nobody remembers, no sign-in copy.
    let mut old = Vault::default();
    old.open("the passphrase that was forgotten long ago", 1, &cheap()).unwrap();
    old.put("gmail", Kind::Login, "pw", 1).unwrap();
    old.lock();
    old.save(&store).unwrap();
    let c = Config::load(Path::new("config")).unwrap();
    let mut d = Daemon::new(&c, &p, None, store, Proactive::new(ProactiveConfig::default()));
    let page = atlas::hublive::reply(&mut d, atlas::server::Action::Hub(atlas::hub::Page::Accounts)).body;
    assert!(page.contains("Unlock it this once"), "the old vault asks once");
    let at = page.find("value=fresh>").expect("the start-afresh form");
    let nonce = page[at..].split("name=nonce value=\"").nth(1).and_then(|r| r.split('"').next()).unwrap().to_string();
    let r = atlas::hublive::reply(
        &mut d,
        atlas::server::Action::Vault {
            what: "fresh".into(),
            old: atlas::server::Secret::new(String::new()),
            new: atlas::server::Secret::new(String::new()),
            again: atlas::server::Secret::new(String::new()),
            nonce,
        },
    );
    assert_eq!(r.status, 303);
    let page = atlas::hublive::reply(&mut d, atlas::server::Action::Hub(atlas::hub::Page::Accounts)).body;
    assert!(page.contains("A new vault is ready"), "said what happened");
    assert!(page.contains("nothing to type"), "and it opens on the sign-in now");
    let aside: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().contains("set-aside")).collect();
    assert_eq!(aside.len(), 1, "the old vault is set aside, not deleted");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn saying_connect_is_the_same_as_pressing_it() {
    use atlas::connecting::{connect_asked, Connect};
    assert_eq!(connect_asked("Connect my Google calendar"), Some(Connect::Google));
    assert_eq!(connect_asked("please connect my outlook"), Some(Connect::Microsoft));
    assert_eq!(connect_asked("connect my YouTube."), Some(Connect::Youtube));
    assert_eq!(connect_asked("sign me into my socials"), Some(Connect::Socials));
    assert_eq!(connect_asked("log me into instagram"), Some(Connect::Socials));
    assert_eq!(connect_asked("what's on my google calendar"), None, "a question, not a request to connect");
    assert_eq!(connect_asked("connect the dots"), None);
}

#[test]
fn one_window_opens_every_site() {
    let launch: Vec<String> = ["--headless=new", "--user-data-dir=data/chrome-profile"].iter().map(|s| s.to_string()).collect();
    let urls: Vec<String> = ["https://www.instagram.com/accounts/login/", "https://www.tiktok.com/login"].iter().map(|s| s.to_string()).collect();
    let a = atlas::browser::sign_in_window_args_for(&launch, &urls);
    assert!(!a.iter().any(|x| x.starts_with("--headless")));
    assert!(urls.iter().all(|u| a.contains(u)), "{a:?}");
}
