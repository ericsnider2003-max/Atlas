//! The vault passphrase, taking a handover back, starting a household, the
//! household key and freeing disk space — all through the hub, with no
//! terminal (27 Sep 2026).
//!
//! Until this, each of those answered with a command. The one that mattered
//! most: a person who never opens a terminal could hand their laptop over and
//! then never take it back, because the only place to set a passphrase — and
//! the only place to type one — was `atlas vault` and `atlas handover back`.
//!
//! Everything here goes through the same door a browser does: a request is
//! routed by `server::route` and answered by `hublive::reply` on a real
//! `Daemon`. Its own test target, because it drives the install's vault and
//! handover, which `roots::install_state()` fixes once per process.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::handover::{Handover, Stance};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::server::{Action, Reply, Request, Secret};
use atlas::store::Store;
use atlas::vault::{State, Vault, VaultConfig};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, Once};

const PASSPHRASE: &str = "the one thing you actually know";

fn alone() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// The install, set before `roots` is first asked. Home and temp point into
/// it too, so "Look for space" walks a folder this test made.
fn home() -> PathBuf {
    static ONCE: Once = Once::new();
    let p = std::env::temp_dir().join(format!("atlas-hub-way-back-{}", std::process::id()));
    ONCE.call_once(|| {
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(p.join("data").join("state")).unwrap();
        std::fs::create_dir_all(p.join("home")).unwrap();
        std::fs::create_dir_all(p.join("temp")).unwrap();
        std::env::set_var("ATLAS_HOME", &p);
        std::env::set_var("HOME", p.join("home"));
        std::env::set_var("USERPROFILE", p.join("home"));
        std::env::set_var("TEMP", p.join("temp"));
        for v in ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"] {
            std::env::remove_var(v);
        }
    });
    p
}

fn install() -> Store {
    home();
    atlas::roots::install_state()
}

/// A fresh copy of the shipped config for one test, which is also where the
/// hub writes settings (`ATLAS_CONFIG`) and what the daemon watches.
fn config(tag: &str) -> PathBuf {
    let dir = home().join(format!("config-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for e in std::fs::read_dir("config").unwrap().flatten() {
        let p = e.path();
        if p.is_file() && p.extension().and_then(|x| x.to_str()) == Some("yaml") && p.file_name().unwrap() != "settings.yaml" {
            std::fs::copy(&p, dir.join(p.file_name().unwrap())).unwrap();
        }
    }
    std::env::set_var("ATLAS_CONFIG", &dir);
    dir
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str, dir: &Path) -> Daemon<'a> {
    let store = home().join(format!("person-{tag}"));
    let _ = std::fs::remove_dir_all(&store);
    std::fs::create_dir_all(&store).unwrap();
    Daemon::new(c, p, None, Store::new(store), Proactive::new(ProactiveConfig::default())).watch_settings(dir.to_path_buf())
}

fn empty_vault() {
    let mut v = Vault::default();
    v.lock();
    v.save(&install()).unwrap();
}

fn a_vault_with_a_passphrase() {
    let mut v = Vault::default();
    v.open(PASSPHRASE, 100, &VaultConfig::default()).unwrap();
    v.lock();
    v.save(&install()).unwrap();
}

fn handed_over(yes: bool) {
    let mut h = Handover::default();
    if yes {
        h.hand_over("", 1_000);
    }
    h.save(&install()).unwrap();
}

fn post(path: &str, body: &str) -> Action {
    let r = Request {
        method: "POST".into(),
        path: path.into(),
        query: String::new(),
        token: Some("x".repeat(24)),
        token_from_url: false,
        body: body.into(),
    };
    atlas::server::route(&r).unwrap_or_else(|| panic!("{path} with {body} reached nothing"))
}

fn enc(s: &str) -> String {
    atlas::research::urlencode(s)
}

/// A page, with the apostrophes `hub::esc` encodes read back as written.
fn page(d: &mut Daemon, p: atlas::hub::Page) -> String {
    let r = atlas::hublive::reply(d, Action::Hub(p));
    assert_eq!(r.status, 200, "{}", r.body);
    r.body.replace("&#39;", "'")
}

/// The one-time mark in the vault section's forms.
fn mark(html: &str) -> String {
    let at = html.find("name=nonce value=\"").expect("the vault form carries no one-time mark") + 18;
    html[at..].split('"').next().unwrap().to_string()
}

/// Every reply in these journeys, checked for the thing this work removed.
fn no_command(r: &Reply) {
    assert!(!r.body.contains("atlas household"), "a command in a hub answer: {}", r.body);
    assert!(!r.body.contains("atlas vault"), "a command in a hub answer: {}", r.body);
    assert!(!r.body.contains("atlas handover"), "a command in a hub answer: {}", r.body);
    assert!(!r.body.contains("atlas sync"), "a command in a hub answer: {}", r.body);
}

// ---------- the vault ----------

#[test]
fn a_first_passphrase_set_in_the_hub_shows_its_recovery_key_once_and_never_in_an_address() {
    let _l = alone();
    empty_vault();
    handed_over(false);
    let dir = config("first");
    let c = Config::load(&dir).unwrap();
    let p = plat();
    let mut d = daemon(&c, &p, "first", &dir);

    let html = page(&mut d, atlas::hub::Page::Accounts);
    assert!(html.contains("id=vault"), "no vault section on the Accounts page");
    assert!(html.contains("Set the passphrase"));
    assert!(html.contains("type=password") && html.contains("autocomplete=new-password"));
    no_command(&Reply::html(html.clone()));
    let n = mark(&html);

    let body = format!("what=set&new={}&again={}&nonce={n}", enc(PASSPHRASE), enc(PASSPHRASE));
    let r = atlas::hublive::reply(&mut d, post("/hub/vault", &body));
    assert_eq!(r.status, 303, "a form that changes something answers with a redirect: {}", r.body);
    let location = r.body.clone();

    // Set, kept, and shut again.
    let on_disk = Vault::load(&install());
    assert!(on_disk.has_a_passphrase(), "nothing was set");
    assert!(on_disk.has_a_recovery_key(), "no recovery key was made with it");
    assert_eq!(d.vault.state(), State::Sealed, "the vault was left open");

    // The key, on the page, once.
    let shown = page(&mut d, atlas::hub::Page::Accounts);
    let at = shown.find("write this down now").expect("the recovery key was not shown");
    let code_at = shown[at..].find("<code>").unwrap() + at + 6;
    let code = shown[code_at..].split("</code>").next().unwrap().to_string();
    assert!(code.len() >= 24, "that isn't a recovery key: {code}");
    assert!(!location.contains(&code), "the recovery key went into an address: {location}");
    assert!(shown.contains("I've written it down"));
    let again = page(&mut d, atlas::hub::Page::Accounts);
    assert!(!again.contains(&code), "the recovery key was shown a second time");

    // And it is a real key: it opens the vault.
    let mut v = Vault::load(&install());
    v.open_with_recovery_key(&code, 200, &VaultConfig::default()).expect("the key shown does not open the vault");

    // A refresh re-sends the same form: answered, not acted on.
    let r = atlas::hublive::reply(&mut d, post("/hub/vault", &body));
    assert_eq!(r.status, 303);
    assert!(page(&mut d, atlas::hub::Page::Accounts).contains("already been sent"));

    // Changing it takes the current one.
    let html = page(&mut d, atlas::hub::Page::Accounts);
    assert!(html.contains("Change the passphrase") && html.contains("autocomplete=current-password"));
    let wrong = format!("what=change&old={}&new={}&again={}&nonce={}", enc("not the passphrase at all"), enc("a brand new sentence to keep"), enc("a brand new sentence to keep"), mark(&html));
    atlas::hublive::reply(&mut d, post("/hub/vault", &wrong));
    let mut v = Vault::load(&install());
    assert!(v.open(PASSPHRASE, 300, &VaultConfig::default()).is_ok(), "a wrong current passphrase changed it");
}

#[test]
fn a_first_passphrase_cannot_be_set_in_the_hub_while_handed_over() {
    let _l = alone();
    empty_vault();
    handed_over(true);
    let dir = config("refused");
    let c = Config::load(&dir).unwrap();
    let p = plat();
    let mut d = daemon(&c, &p, "refused", &dir);

    let html = page(&mut d, atlas::hub::Page::Accounts);
    assert!(!html.contains("Set the passphrase"), "the form that hands out the way back is offered");
    assert!(html.contains("Not while this is handed over"), "{html}");

    // Posted anyway, as someone who knows the form would: first with a mark
    // nobody handed out, then with a real one -- refused by the rule, not
    // just by the mark.
    let body = format!("what=set&new={}&again={}&nonce=made-up", enc(PASSPHRASE), enc(PASSPHRASE));
    atlas::hublive::reply(&mut d, post("/hub/vault", &body));
    assert!(!Vault::load(&install()).has_a_passphrase(), "a made-up mark set a passphrase");
    let real = d.shown_once.mark();
    let body = format!("what=set&new={}&again={}&nonce={real}", enc(PASSPHRASE), enc(PASSPHRASE));
    atlas::hublive::reply(&mut d, post("/hub/vault", &body));
    assert!(!Vault::load(&install()).has_a_passphrase(), "the hub set the first passphrase while handed over");
    assert!(page(&mut d, atlas::hub::Page::Accounts).contains("Not while this is handed over"));

    // Every page says it is handed over and where it is taken back.
    let status = page(&mut d, atlas::hub::Page::Status);
    assert!(status.contains("/hub/accounts#vault"), "no way back from the Status page");
    handed_over(false);
    let status = page(&mut d, atlas::hub::Page::Status);
    assert!(!status.contains("This is handed over"), "the banner stayed after it was taken back");
}

#[test]
fn taking_it_back_in_the_hub_needs_the_right_passphrase_and_leaves_the_vault_locked() {
    let _l = alone();
    a_vault_with_a_passphrase();
    handed_over(true);
    let dir = config("back");
    let c = Config::load(&dir).unwrap();
    let p = plat();
    let mut d = daemon(&c, &p, "back", &dir);

    let html = page(&mut d, atlas::hub::Page::Accounts);
    assert!(html.contains("Take it back"));
    assert!(html.contains("autocomplete=current-password"));

    // The owner had the vault open before handing it over. A wrong passphrase
    // must not ride on that earlier proof.
    d.vault.open(PASSPHRASE, 500, &VaultConfig::default()).unwrap();
    assert!(d.vault.proved_it());
    let wrong = format!("what=back&phrase={}&nonce={}", enc("somebody else's guess here"), mark(&html));
    let r = atlas::hublive::reply(&mut d, post("/hub/vault", &wrong));
    assert_eq!(r.status, 303);
    assert!(!r.body.contains("guess"), "the passphrase went into the address: {}", r.body);
    assert_eq!(Handover::load(&install()).stance, Stance::HandedOver, "a wrong passphrase took it back");
    assert_eq!(d.vault.state(), State::Sealed);

    let html = page(&mut d, atlas::hub::Page::Accounts);
    assert!(html.contains("isn't the passphrase") || html.contains("that isn't"), "the refusal was not said: {html}");
    let right = format!("what=back&phrase={}&nonce={}", enc(PASSPHRASE), mark(&html));
    atlas::hublive::reply(&mut d, post("/hub/vault", &right));
    assert_eq!(Handover::load(&install()).stance, Stance::Yours, "the right passphrase did not take it back");
    assert_eq!(d.vault.state(), State::Sealed, "the vault was left open after proving who typed");
    let html = page(&mut d, atlas::hub::Page::Accounts);
    assert!(html.contains("Yours again"), "{html}");
    assert!(html.contains("tried and refused 1 time"), "the wrong try was not reported: {html}");
}

#[test]
fn a_secret_never_prints_itself() {
    let actions = vec![
        post("/hub/vault", "what=set&new=hunter2-hunter2-x&again=hunter2-hunter2-x&nonce=n"),
        post("/hub/vault", "what=back&phrase=hunter2-hunter2-x&nonce=n"),
        post("/hub/vault", "what=recovery&old=hunter2-hunter2-x&nonce=n"),
        post("/hub/sync", "what=set-key&phrase=hunter2-hunter2-x&replace=yes"),
    ];
    for a in &actions {
        let shown = format!("{a:?}");
        assert!(!shown.contains("hunter2"), "a secret printed itself: {shown}");
        assert!(shown.contains("Secret("), "{shown}");
        assert!(a.carries_a_secret(), "{shown} is not held to the private line");
    }
    assert!(format!("{:?}", Secret::new("hunter2")) == "Secret(\u{2026})");
    // And they are what the forms send.
    match &actions[3] {
        Action::SyncKeySet { phrase, replace } => {
            assert_eq!(phrase.reveal(), "hunter2-hunter2-x");
            assert!(*replace);
        }
        other => panic!("{other:?}"),
    }
    assert!(!post("/hub/sync", "what=init&name=Home&device=x").carries_a_secret());
}

#[test]
fn passphrases_only_travel_on_this_machine_or_tailscale() {
    use atlas::server::private_line;
    for (ip, ok) in [
        ("127.0.0.1", true),
        ("::1", true),
        ("100.101.2.3", true),
        ("100.64.0.1", true),
        ("100.127.255.254", true),
        ("fd7a:115c:a1e0::1", true),
        ("::ffff:100.101.2.3", true),
        ("192.168.1.5", false),
        ("10.0.0.2", false),
        ("100.128.0.1", false),
        ("100.63.255.255", false),
        ("8.8.8.8", false),
        ("fd7a:115c:a1e1::1", false),
        ("::ffff:192.168.1.5", false),
    ] {
        assert_eq!(private_line(ip.parse().unwrap()), ok, "{ip}");
    }
    // Wired where the address and the action meet: the check is in the
    // connection handler, before the action reaches Atlas.
    let src = with_split_children("src/server.rs");
    // Since 28 Sep 2026 every connection (threaded or polled) is read and
    // checked in `read_asked`, which hands the action on as `Asked`.
    let at = src.find("fn read_asked(").expect("read_asked");
    let body = &src[at..];
    let check = body.find("action.carries_a_secret()").expect("the connection handler never asks");
    let handed = body.find("Some(action) => {\n                return Ok(Some(Asked {").expect("handed on");
    assert!(check < handed, "the check comes after the action was handled");
    assert!(body[check..handed].contains("private_line(a.ip())"));
}

// ---------- the Sync page ----------

#[test]
fn a_fresh_install_goes_folder_then_household_then_pairing_entirely_in_the_hub() {
    let _l = alone();
    handed_over(false);
    let dir = config("fresh");
    let c = Config::load(&dir).unwrap();
    let p = plat();
    let mut d = daemon(&c, &p, "fresh", &dir);
    assert!(d.tools_cfg().sync.folder.trim().is_empty(), "the shipped config has a sync folder");

    let mut all: Vec<Reply> = Vec::new();
    let sync = atlas::hub::Page::Sync;
    let html = page(&mut d, sync);
    assert!(html.contains("Where your devices meet"), "no way to choose a folder");
    all.push(Reply::html(html));

    // 1. The folder, through the settings the hub keeps.
    let meet = home().join("meet");
    let r = atlas::hublive::reply(&mut d, post("/hub/sync-setup", &format!("folder={}&device={}", enc(&meet.display().to_string()), enc("study laptop"))));
    assert_eq!(r.status, 303);
    all.push(r);
    assert_eq!(d.tools_cfg().sync.folder, meet.display().to_string(), "the folder did not take");
    assert_eq!(d.tools_cfg().household.device_name, "study laptop");
    // Kept, not just held: a fresh read of the config has it.
    let reread = Config::load(&dir).unwrap().tools.unwrap();
    assert_eq!(reread.sync.folder, meet.display().to_string(), "settings.yaml did not keep the folder");
    assert_eq!(reread.household.device_name, "study laptop");

    // 2. A household, started here.
    let html = page(&mut d, sync);
    assert!(html.contains("Start one here"), "{html}");
    assert!(html.contains("value=\"study laptop\""), "the device name is not offered");
    assert!(html.contains("name=code"), "the join form is gone");
    all.push(Reply::html(html));

    // The name left out: said what's missing, nothing made.
    let r = atlas::hublive::reply(&mut d, post("/hub/sync", "what=init&name=&device=&key=yes"));
    all.push(r);
    let html = page(&mut d, sync);
    assert!(html.contains("Give the household a name"), "{html}");
    assert!(!atlas::household::Household::load(&d.store).is_set());

    let r = atlas::hublive::reply(&mut d, post("/hub/sync", "what=init&name=Home&device=&key=yes"));
    all.push(r);
    let house = atlas::household::Household::load(&d.store);
    assert!(house.is_set());
    assert_eq!(house.devices, vec!["study laptop".to_string()], "the device name was not used");
    let kept: atlas::sync::KeptKey = d.store.load(atlas::sync::KEY_FILE);
    assert!(kept.is_set(), "\"make a household key too\" made none");
    let html = page(&mut d, sync);
    assert!(html.contains("Started Home. Invite your other devices below."), "{html}");
    assert!(html.contains("Invite a device") && html.contains("Use a key from another device"));
    all.push(Reply::html(html));

    // A second household on the same device is refused.
    atlas::hublive::reply(&mut d, post("/hub/sync", "what=init&name=Other&device=&key=yes"));
    assert!(page(&mut d, sync).contains("already belongs to Home"));
    assert_eq!(atlas::household::Household::load(&d.store).id, house.id);

    // 3. Inviting the next device.
    let r = atlas::hublive::reply(&mut d, post("/hub/sync", "what=pair"));
    all.push(r);
    let html = page(&mut d, sync);
    // 30 Sep 2026: fifteen minutes (`household::INVITE_WAIT_SECS`).
    let lead = "Type this on the other machine within fifteen minutes: ";
    let at = html.find(lead).expect("no code") + lead.len();
    let code: String = html[at..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    assert!(code.len() >= 8, "{code}");
    all.push(Reply::html(html));

    // The other device joins with it, also through its own hub.
    let c2 = Config::load(&dir).unwrap();
    let mut other = daemon(&c2, &p, "fresh-other", &dir);
    let r = atlas::hublive::reply(&mut other, post("/hub/sync", &format!("what=join&code={}&device=phone", enc(&code))));
    all.push(r);
    let joined = atlas::household::Household::load(&other.store);
    let said = page(&mut other, sync);
    assert_eq!(joined.id, house.id, "the other device did not join the same household (code {code}): {}", &said[said.find("<main").unwrap_or(0)..]);
    let theirs: atlas::sync::KeptKey = other.store.load(atlas::sync::KEY_FILE);
    assert_eq!(theirs.phrase().unwrap(), kept.phrase().unwrap(), "the key did not come with it");
    all.push(Reply::html(page(&mut other, sync)));

    for r in &all {
        no_command(r);
    }
}

#[test]
fn nothing_about_the_household_changes_while_handed_over() {
    let _l = alone();
    handed_over(true);
    let dir = config("hh-handed");
    let c = Config::load(&dir).unwrap();
    let p = plat();
    let mut d = daemon(&c, &p, "hh-handed", &dir);
    atlas::hublive::reply(&mut d, post("/hub/sync", "what=init&name=Mine&device=x&key=yes"));
    assert!(!atlas::household::Household::load(&d.store).is_set(), "a household was started while handed over");
    assert!(page(&mut d, atlas::hub::Page::Sync).contains("Not while this is handed over"));

    let phrase = atlas::sync::new_key_phrase();
    atlas::hublive::reply(&mut d, post("/hub/sync", &format!("what=set-key&phrase={}", enc(&phrase))));
    let kept: atlas::sync::KeptKey = d.store.load(atlas::sync::KEY_FILE);
    assert!(!kept.is_set(), "a household key was set while handed over");

    // Inviting would hand the key to whatever device typed the code.
    atlas::hublive::reply(&mut d, post("/hub/sync", "what=pair"));
    assert!(!page(&mut d, atlas::hub::Page::Sync).contains("Type this on the other machine"));
    handed_over(false);
}

#[test]
fn a_key_from_another_device_is_checked_and_does_not_replace_one_unasked() {
    let _l = alone();
    handed_over(false);
    let dir = config("key");
    let c = Config::load(&dir).unwrap();
    let p = plat();
    let mut d = daemon(&c, &p, "key", &dir);

    // A mistyped phrase is not kept.
    atlas::hublive::reply(&mut d, post("/hub/sync", "what=set-key&phrase=hello"));
    let kept: atlas::sync::KeptKey = d.store.load(atlas::sync::KEY_FILE);
    assert!(!kept.is_set(), "a bad phrase was kept");
    assert!(page(&mut d, atlas::hub::Page::Sync).contains("isn't a household key"));

    let first = atlas::sync::new_key_phrase();
    atlas::hublive::reply(&mut d, post("/hub/sync", &format!("what=set-key&phrase={}", enc(&first))));
    let kept: atlas::sync::KeptKey = d.store.load(atlas::sync::KEY_FILE);
    assert_eq!(kept.phrase().unwrap(), first);

    // Another one, unticked: kept as it was.
    let second = atlas::sync::new_key_phrase();
    atlas::hublive::reply(&mut d, post("/hub/sync", &format!("what=set-key&phrase={}", enc(&second))));
    let kept: atlas::sync::KeptKey = d.store.load(atlas::sync::KEY_FILE);
    assert_eq!(kept.phrase().unwrap(), first, "an existing key was replaced without \"replace my key\"");
    assert!(page(&mut d, atlas::hub::Page::Sync).contains("replace my key"));

    atlas::hublive::reply(&mut d, post("/hub/sync", &format!("what=set-key&phrase={}&replace=yes", enc(&second))));
    let kept: atlas::sync::KeptKey = d.store.load(atlas::sync::KEY_FILE);
    assert_eq!(kept.phrase().unwrap(), second, "ticking replace did not replace it");
}

// ---------- Free up space ----------

fn folder(name: &str) -> PathBuf {
    let p = home().join("home").join("space").join(name);
    std::fs::create_dir_all(&p).unwrap();
    std::fs::write(p.join("junk.bin"), vec![0u8; 1024]).unwrap();
    p
}

#[test]
fn only_what_was_ticked_and_atlas_may_move_goes_to_the_trash() {
    use atlas::reclaim::{Candidate, Kind};
    let _l = alone();
    handed_over(false);
    let dir = config("space");
    let c = Config::load(&dir).unwrap();
    let p = plat();
    let mut d = daemon(&c, &p, "space", &dir);

    let ticked = folder("ticked-cache");
    let unticked = folder("unticked-cache");
    let yours = folder("your-big-folder");
    let survey = atlas::hubvault::Survey {
        at: atlas::store::now(),
        found: vec![
            Candidate { path: ticked.clone(), size_mb: 900, kind: Kind::PackageCache, age_days: 90 },
            Candidate { path: unticked.clone(), size_mb: 800, kind: Kind::Temp, age_days: 30 },
            Candidate { path: yours.clone(), size_mb: 5000, kind: Kind::BigFolder, age_days: 0 },
        ],
    };
    d.store.save(atlas::hubvault::SURVEY, &survey).unwrap();

    let html = page(&mut d, atlas::hub::Page::Status);
    assert!(html.contains("Free up space") && html.contains("Move chosen to the trash"));
    assert!(html.contains(&format!("value=\"{}\"", atlas::hub::esc(&ticked.display().to_string()))), "no box for what I may move");
    assert!(!html.contains(&format!("value=\"{}\"", atlas::hub::esc(&yours.display().to_string()))), "a box for what is yours to judge");

    // Ticked: one of mine, one of yours (a hand-made form), and a path the
    // survey never found.
    let stranger = folder("never-surveyed");
    let body = format!(
        "what=move&pick={}&pick={}&pick={}",
        enc(&ticked.display().to_string()),
        enc(&yours.display().to_string()),
        enc(&stranger.display().to_string())
    );
    let r = atlas::hublive::reply(&mut d, post("/hub/reclaim", &body));
    assert_eq!(r.status, 303);
    assert!(!ticked.exists(), "the ticked cache was not moved");
    assert!(unticked.exists(), "an unticked one was moved");
    assert!(yours.exists(), "something Atlas may not move was moved");
    assert!(stranger.exists(), "a path the survey never found was moved");
    let html = page(&mut d, atlas::hub::Page::Status);
    assert!(html.contains("Moved 1 to the trash"), "{html}");
}

#[test]
fn looking_for_space_runs_on_the_crew_and_lands_on_the_status_page() {
    let _l = alone();
    handed_over(false);
    let dir = config("look");
    let c = Config::load(&dir).unwrap();
    let p = plat();
    let mut d = daemon(&c, &p, "look", &dir);

    let r = atlas::hublive::reply(&mut d, post("/hub/reclaim", "what=look"));
    assert_eq!(r.status, 303);
    let html = page(&mut d, atlas::hub::Page::Status);
    assert!(html.contains("Looking through the disk now"), "{html}");

    let mut landed = false;
    for _ in 0..200 {
        d.tick(atlas::store::now());
        let s: atlas::hubvault::Survey = d.store.load(atlas::hubvault::SURVEY);
        if s.at > 0 {
            landed = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(landed, "the survey never came back from the crew");
    let html = page(&mut d, atlas::hub::Page::Status);
    assert!(html.contains("Last looked"), "{html}");
    assert!(html.contains("Look for space"), "the button did not come back");
}

// ---------- said everywhere else ----------

#[test]
fn nothing_the_hub_says_about_these_names_a_command() {
    let src = with_split_children("src/hublive.rs");
    assert!(!src.contains("`atlas household init"), "the Sync page still answers with a command");
    let hub = with_split_children("src/hub.rs");
    assert!(!hub.contains("<code>atlas household join</code>"), "the invite text still names a command");
    let page = sync_page_unplaced(true, "D:/atlas", None, None, None, "");
    assert!(!page.contains("atlas household"));
}


/// The Sync page drawn without knowing where this device stands -- what
/// `hub::sync_page` did before the running Atlas moved to `sync_page_with`
/// and the wrapper, called by nothing else, was folded in (28 Sep 2026).
fn sync_page_unplaced(sealing: bool, folder: &str, phrase: Option<&str>, card: Option<&str>, last: Option<&str>, this_device: &str) -> String {
    atlas::hub::sync_page_with(
        sealing,
        folder,
        phrase,
        card,
        last,
        this_device,
        &atlas::hub::SyncView { house: atlas::hub::HouseView::Unknown, suggested_folder: None },
    )
}

/// A source file and every child module split out beside it (`src/hub.rs`
/// plus `src/hub/*.rs`, recursively): audit Q6 moved code into children
/// without changing what the module is.
fn with_split_children(path: &str) -> String {
    fn walk(dir: &Path, out: &mut String) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|e| e == "rs") {
                out.push('\n');
                out.push_str(&std::fs::read_to_string(&p).unwrap());
            }
        }
    }
    let mut text = std::fs::read_to_string(path).unwrap();
    walk(&Path::new(path).with_extension(""), &mut text);
    text
}
