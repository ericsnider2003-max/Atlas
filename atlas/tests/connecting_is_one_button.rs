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

/// A private copy of config/ for one test: Settings forms write
/// settings.yaml there, not into the checkout (9 Oct 2026).
fn private_config(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("atlas-onebutton-config-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy_tree(Path::new("config"), &dir);
    let _ = std::fs::remove_file(dir.join("settings.yaml"));
    dir
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if src.is_dir() {
            copy_tree(&src, &dst);
        } else {
            std::fs::copy(&src, &dst).unwrap();
        }
    }
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
fn an_old_vault_is_never_a_chore_on_the_page() {
    stand_in();
    // Off Windows (or handed over), where it can't move: asked once, and no
    // "start again" chore folded away under it (6 Oct 2026).
    let v = atlas::hub::VaultView { needs_its_passphrase_once: true, has_passphrase: true, nonce: "n".into(), ..Default::default() };
    let html = atlas::hub::vault_section(&v);
    assert!(html.contains("Unlock it this once"), "{html}");
    assert!(!html.contains("Start a new vault") && !html.contains("value=fresh"), "{html}");
    // Moved on: it opens on the sign-in, and the old one is named, not lost.
    let v = atlas::hub::VaultView { opens_on_login: true, set_aside: vec!["atlas-release-key".into()], nonce: "n".into(), ..Default::default() };
    let html = atlas::hub::vault_section(&v);
    assert!(html.contains("nothing to type"), "{html}");
    assert!(html.contains("Your old vault") && html.contains("atlas-release-key") && html.contains("value=bring"), "{html}");
    let v = atlas::hub::VaultView { opens_on_login: true, nonce: "n".into(), ..Default::default() };
    let html = atlas::hub::vault_section(&v);
    assert!(!html.contains("Your old vault"), "nothing set aside, nothing said: {html}");
    let shown = &html[..html.find("<details").expect("the passphrase is folded away")];
    assert!(!shown.contains("type=password"), "no passphrase box in sight: {shown}");
}

fn nonce_for(page: &str, what: &str) -> String {
    let at = page.find(&format!("value={what}>")).unwrap_or_else(|| panic!("the {what} form in {page}"));
    page[at..].split("name=nonce value=\"").nth(1).and_then(|r| r.split('"').next()).unwrap().to_string()
}

fn post_to_vault(d: &mut Daemon, what: &str, old: &str, nonce: String) -> u16 {
    atlas::hublive::reply(
        d,
        atlas::server::Action::Vault {
            what: what.into(),
            old: atlas::server::Secret::new(old.to_string()),
            new: atlas::server::Secret::new(String::new()),
            again: atlas::server::Secret::new(String::new()),
            nonce,
        },
    )
    .status
}

#[test]
fn an_old_vault_moves_to_the_sign_in_by_itself_and_nothing_is_lost() {
    stand_in();
    let (dir, p) = daemon("moves");
    let store = Store::new(dir.clone());
    // A vault made the old way: a passphrase nobody remembers, no sign-in
    // copy -- and in it, the one thing that can't be made again.
    let phrase = "the passphrase that was forgotten long ago";
    let mut old = Vault::default();
    old.open(phrase, 1, &cheap()).unwrap();
    old.put("gmail", Kind::Login, "pw", 1).unwrap();
    old.put(atlas::release::RELEASE_KEY_NAME, Kind::ApiKey, &"ab".repeat(32), 1).unwrap();
    old.lock();
    old.save(&store).unwrap();
    let bytes_before = std::fs::read(dir.join("vault.json")).unwrap();
    let c = Config::load(Path::new("config")).unwrap();
    let mut d = Daemon::new(&c, &p, None, store.clone(), Proactive::new(ProactiveConfig::default()));

    // Nothing pressed: looking at the page is enough.
    let page = atlas::hublive::reply(&mut d, atlas::server::Action::Hub(atlas::hub::Page::Accounts)).body;
    assert!(!page.contains("Unlock it this once"), "no question asked");
    assert!(page.contains("nothing to type"), "it opens on the sign-in now");
    assert!(page.contains("Your old vault") && page.contains("gmail"), "and the old one is named");
    assert_eq!(std::fs::read(dir.join("vault-set-aside.json")).unwrap(), bytes_before, "set aside byte for byte");
    let now_here = Vault::load(&store);
    assert!(now_here.sealed_to_this_login() && !now_here.has_a_passphrase());
    assert_eq!(now_here.set_aside, vec!["gmail".to_string(), atlas::release::RELEASE_KEY_NAME.to_string()]);

    // A second release key would strand every copy handed out.
    let mut v = Vault::load(&store);
    v.open_unattended(2).unwrap();
    assert_eq!(atlas::release::make_release_key(&mut v, 2).unwrap_err(), atlas::release::IN_THE_OLD_VAULT);

    // The wrong passphrase brings nothing.
    let n = nonce_for(&page, "bring");
    assert_eq!(post_to_vault(&mut d, "bring", "not the passphrase at all, sorry", n), 303);
    let page = atlas::hublive::reply(&mut d, atlas::server::Action::Hub(atlas::hub::Page::Accounts)).body;
    assert!(page.contains("isn&#39;t the old vault") || page.contains("isn't the old vault"), "{page}");
    assert!(dir.join("vault-set-aside.json").exists());

    // The right one brings everything across, and retires the old file.
    let n = nonce_for(&page, "bring");
    assert_eq!(post_to_vault(&mut d, "bring", phrase, n), 303);
    let page = atlas::hublive::reply(&mut d, atlas::server::Action::Hub(atlas::hub::Page::Accounts)).body;
    assert!(page.contains("Brought across: gmail"), "{page}");
    assert!(!page.contains("Your old vault"));
    let mut v = Vault::load(&store);
    assert!(v.set_aside.is_empty());
    v.open_unattended(3).unwrap();
    assert_eq!(v.get("gmail", 3).unwrap(), "pw");
    assert_eq!(v.get(atlas::release::RELEASE_KEY_NAME, 3).unwrap(), "ab".repeat(32));
    assert!(!dir.join("vault-set-aside.json").exists());
    let retired = std::fs::read_dir(&dir).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("vault-set-aside-brought-in-")).count();
    assert_eq!(retired, 1, "kept, under another name, never deleted");
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

fn post_social(d: &mut Daemon, fields: &[(&str, &str)]) -> String {
    let r = atlas::hublive::reply(
        d,
        atlas::server::Action::HubPost {
            path: "/hub/social".into(),
            fields: fields.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
        },
    );
    atlas::hub::urldecode(&r.body)
}

/// N5 leftovers (8 Oct 2026): Instagram, Threads, Facebook and TikTok sign-ins
/// had a "Keep it" and no way to take them away again.
#[test]
fn an_instagram_threads_facebook_or_tiktok_sign_in_can_be_disconnected() {
    stand_in();
    let (dir, p) = daemon("takeaway");
    let c = Config::load(Path::new("config")).unwrap();
    let mut d = Daemon::new(&c, &p, None, Store::new(dir.clone()), Proactive::new(ProactiveConfig::default())).watch_settings(private_config("takeaway"));
    // Nothing kept: no button, and the post says so rather than pretending.
    let html = atlas::hublive::reply(&mut d, atlas::server::Action::Hub(atlas::hub::Page::Social)).body;
    assert!(!html.contains("value='token-disconnect'"), "a Disconnect button for something not kept");
    let back = post_social(&mut d, &[("what", "token-disconnect"), ("name", "instagram")]);
    assert!(back.contains("nothing to take away"), "{back}");

    for (name, kept) in [("instagram", "Instagram token"), ("threads", "Threads token"), ("facebook", "Facebook Page token")] {
        let back = post_social(&mut d, &[("what", "key"), ("name", name), ("secret", "EAAB-not-real")]);
        assert!(back.contains("Kept your"), "{back}");
        let html = atlas::hublive::reply(&mut d, atlas::server::Action::Hub(atlas::hub::Page::Social)).body;
        assert!(html.contains(&format!("name=name value={name}")), "{kept} is kept and has no Disconnect button");
        let back = post_social(&mut d, &[("what", "token-disconnect"), ("name", name)]);
        assert!(back.contains("disconnected") && back.contains("gone from the vault"), "{back}");
        assert!(back.contains("Business Integrations"), "Meta's own page is named, since it gives Atlas no way to end the token: {back}");
        let mut v = Vault::load(&Store::new(dir.clone()));
        v.open_unattended(1).unwrap();
        let secret = match name { "instagram" => atlas::social::VAULT_INSTAGRAM, "threads" => atlas::social::VAULT_THREADS, _ => atlas::social::VAULT_FACEBOOK };
        assert!(v.get(secret, 1).is_err(), "{name} is still in the vault");
        let again = post_social(&mut d, &[("what", "token-disconnect"), ("name", name)]);
        assert!(again.contains("nothing to take away"), "{again}");
    }

    // TikTok: kept as its sign-in record (no refresh token, so nothing to ask TikTok to end).
    let rec = r#"{"client_key":"ck","client_secret":"cs","redirect":"https://x.example/r"}"#;
    d.vault.put(atlas::social::VAULT_TIKTOK, Kind::ApiKey, rec, 1).unwrap();
    d.vault.save(&d.vault_home).unwrap();
    let back = post_social(&mut d, &[("what", "token-disconnect"), ("name", "tiktok")]);
    assert!(back.contains("TikTok is disconnected"), "{back}");
    let back = post_social(&mut d, &[("what", "token-disconnect"), ("name", "linkedin")]);
    assert!(back.contains("isn't a sign-in I keep"), "{back}");
    let _ = std::fs::remove_dir_all(dir);
}

/// N5 leftovers (9 Oct 2026): Bluesky is a handle (and maybe a posting app
/// password), and had no Disconnect.
#[test]
fn bluesky_can_be_disconnected_handle_and_app_password() {
    stand_in();
    let (dir, p) = daemon("bskytakeaway");
    let c = Config::load(Path::new("config")).unwrap();
    let mut d = Daemon::new(&c, &p, None, Store::new(dir.clone()), Proactive::new(ProactiveConfig::default())).watch_settings(private_config("bsky"));
    let back = post_social(&mut d, &[("what", "bluesky-disconnect")]);
    assert!(back.contains("nothing to take away"), "{back}");
    let html = atlas::hublive::reply(&mut d, atlas::server::Action::Hub(atlas::hub::Page::Social)).body;
    assert!(!html.contains("value='bluesky-disconnect'"), "a Disconnect button for something not connected");

    post_social(&mut d, &[("what", "bluesky-handle"), ("handle", "eric.bsky.social")]);
    let back = post_social(&mut d, &[("what", "key"), ("name", "bluesky"), ("secret", "abcd-efgh-ijkl-mnop")]);
    assert!(back.contains("Kept your"), "{back}");
    let html = atlas::hublive::reply(&mut d, atlas::server::Action::Hub(atlas::hub::Page::Social)).body;
    let at = html.find("Bluesky").unwrap_or(0);
    assert!(html.contains("value='bluesky-disconnect'"), "connected Bluesky has no Disconnect button: {}", &html[at.saturating_sub(100)..(at + 900).min(html.len())]);

    let back = post_social(&mut d, &[("what", "bluesky-disconnect")]);
    assert!(back.contains("Bluesky is disconnected"), "{back}");
    let mut v = Vault::load(&Store::new(dir.clone()));
    v.open_unattended(1).unwrap();
    assert!(v.get(atlas::social::VAULT_BLUESKY, 1).is_err(), "the app password is still in the vault");
    let html = atlas::hublive::reply(&mut d, atlas::server::Action::Hub(atlas::hub::Page::Social)).body;
    assert!(!html.contains("value='bluesky-disconnect'"), "Disconnect still offered after disconnecting");
    assert!(html.contains("Connect Bluesky") && !html.contains("eric.bsky.social"), "the handle is still set");
    let again = post_social(&mut d, &[("what", "bluesky-disconnect")]);
    assert!(again.contains("nothing to take away"), "{again}");
    let _ = std::fs::remove_dir_all(dir);
}
