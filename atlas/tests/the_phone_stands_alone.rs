//! Atlas on the phone, standing alone, and every hub page usable by everyone.
//!
//! Eric, 26 Sep 2026: "finish building the hub for everything you said isn't
//! built and the phone versions of all of it as we have determined Atlas will
//! be a stand alone on the phones as well … Now we also need to look at
//! accessibility laws for apps and implement all of those as well."
//!
//! What this file holds:
//!
//! - The phone core (`src/mobile.rs`) starts from an empty app folder, serves
//!   the hub on loopback only, and stops when asked.
//! - Every page, the new ones included (Messages, Business Overview, Shared
//!   tasks as Table/Board/Calendar, Clients, Partners, Documents, Sound &
//!   voice, Trusted, Give, Offline, Talk, Help), renders through the real
//!   daemon, and has what WCAG 2.2 AA and EN 301 549 need of its structure:
//!   a language, a skip link, one main landmark, a named nav, the phone's tab
//!   bar, a zoomable viewport, no timed refresh, help in the same place, and
//!   every form control labelled. (Colour contrast, reflow at 320px and the
//!   rest of axe-core's rules were run against the rendered pages in a real
//!   browser on 26 Sep; the numbers are in docs/ACCESSIBILITY.md.)
//! - The forms on those pages do what they say: Give hands a link to the
//!   tray, a share from the phone arrives on Give, Sound & voice settings are
//!   kept, a spoken turn from the phone app comes back marked to be read out
//!   only when Sound & voice allows it.
//! - `/hub/live.json`, which the phone's live activity reads, has its shape.
//! - The installed web app declares its share target and locks no
//!   orientation (WCAG 1.3.4).
//!
//! It runs in its own process because it points ATLAS_HOME at a scratch
//! folder, as the phone app does at its own.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::hub::{Page, NAV};
use atlas::platform::{mock::MockPlatform, Monitor};
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::server::{Action, Reply};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// One scratch home for the whole binary, with the shipped config copied in.
fn home() -> &'static PathBuf {
    static HOME: OnceLock<PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        let h = std::env::temp_dir().join(format!("atlas-phone-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&h);
        std::fs::create_dir_all(h.join("config")).unwrap();
        for f in std::fs::read_dir("config").unwrap().flatten() {
            if f.path().is_file() {
                std::fs::copy(f.path(), h.join("config").join(f.file_name())).unwrap();
            }
        }
        std::env::set_var("ATLAS_HOME", &h);
        h
    })
}

/// The daemon tests share one scratch home; one at a time.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn with_daemon<T>(f: impl FnOnce(&mut Daemon) -> T) -> T {
    let _g = ONE_AT_A_TIME.lock().unwrap_or_else(|p| p.into_inner());
    let h = home();
    let cfg: &'static Config = Box::leak(Box::new(Config::load(&h.join("config")).unwrap()));
    let plat: &'static MockPlatform =
        Box::leak(Box::new(MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])));
    let store_dir = std::env::temp_dir().join(format!("atlas-phone-store-{}-{:?}", std::process::id(), std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&store_dir);
    std::fs::create_dir_all(&store_dir).unwrap();
    std::fs::write(store_dir.join("roster.json"), r#"{"businesses":{"Northwind LLC":["Jordan"]}}"#).unwrap();
    // Watching its settings, as the running Atlas does, so a change on a
    // page is picked up the way it is for real.
    let mut d = Daemon::new(cfg, plat, None, atlas::store::Store::new(store_dir.clone()), Proactive::new(ProactiveConfig::default()))
        .watch_settings(h.join("config"));
    let out = f(&mut d);
    let _ = std::fs::remove_dir_all(&store_dir);
    out
}

fn get(d: &mut Daemon, a: Action) -> Reply {
    atlas::hublive::reply(d, a)
}

fn post(d: &mut Daemon, path: &str, fields: &[(&str, &str)]) -> Reply {
    let fields = fields.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    get(d, Action::HubPost { path: path.into(), fields })
}

// ---------------------------------------------------------------- the phone core

#[test]
fn the_phone_core_serves_the_hub_on_loopback_and_stops_when_asked() {
    let _one = PHONE_CORE.lock().unwrap_or_else(|p| p.into_inner());
    let h = home().clone();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (tx, rx) = std::sync::mpsc::channel();
    let s2 = stop.clone();
    let t = std::thread::spawn(move || atlas::mobile::serve(&h, 0, s2, |u| tx.send(u).unwrap()));
    let url = rx.recv_timeout(std::time::Duration::from_secs(30)).expect("the hub answers");
    assert!(url.starts_with("http://127.0.0.1:") && url.contains("/hub?t="), "loopback, with the token: {url}");
    let (addr, rest) = url.trim_start_matches("http://").split_once('/').unwrap();
    use std::io::{Read, Write};
    let fetch = |req: String| {
        let mut c = std::net::TcpStream::connect(addr).unwrap();
        c.write_all(req.as_bytes()).unwrap();
        let mut got = String::new();
        let _ = c.read_to_string(&mut got);
        got
    };
    // The WebView opens the printed address: the token becomes a cookie.
    let first = fetch(format!("GET /{rest} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"));
    assert!(
        first.contains("Set-Cookie: atlas_hub=") && first.contains("location.replace('/hub')"),
        "the token is traded for a cookie, and the page moves on to the clean address: {}",
        &first[..first.len().min(400)]
    );
    // The phone's live activity reads live.json with the token as a bearer.
    let token = rest.split("t=").nth(1).unwrap().split('&').next().unwrap();
    let live = fetch(format!(
        "GET /hub/live.json HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"
    ));
    assert!(live.starts_with("HTTP/1.1 200") && live.contains("\"waiting\""), "{}", &live[..live.len().min(300)]);
    // So do its widgets, through the app: the glance, with the same token.
    let glance = fetch(format!(
        "GET /hub/glance.json HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"
    ));
    assert!(glance.starts_with("HTTP/1.1 200") && glance.contains("\"lock\""), "{}", &glance[..glance.len().min(300)]);
    let refused = fetch(format!("GET /hub/glance.json HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"));
    assert!(!refused.starts_with("HTTP/1.1 200"), "glance.json without the token: {}", &refused[..refused.len().min(200)]);
    // Without the token, nothing.
    let refused = fetch(format!("GET /hub/live.json HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"));
    assert!(!refused.starts_with("HTTP/1.1 200"), "live.json without the token: {}", &refused[..refused.len().min(200)]);
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    t.join().unwrap().unwrap();
}

/// The two tests that run a whole phone core take turns.
static PHONE_CORE: Mutex<()> = Mutex::new(());

/// 28 Sep 2026: the start waited up to 20 s on the app's main thread, and a
/// slow start left its serving thread running with no address kept -- so
/// the next start began a second Atlas beside the first. Now a start
/// returns at once, the serving thread writes its own address, and a start
/// while one is on its way never begins another.
#[test]
fn starting_twice_while_the_first_is_on_its_way_starts_one_atlas() {
    use atlas::mobile::{phase, start_once, starts, Phase, Started};
    let _one = PHONE_CORE.lock().unwrap_or_else(|p| p.into_inner());
    let h = home().clone();
    let before = starts();
    let t0 = std::time::Instant::now();
    let first = start_once(h.clone(), 0, std::time::Duration::ZERO);
    assert!(t0.elapsed() < std::time::Duration::from_secs(2), "it didn't wait for the hub");
    assert!(matches!(first, Started::Starting | Started::Running(_)), "{first:?}");
    let second = start_once(h.clone(), 0, std::time::Duration::ZERO);
    assert!(matches!(second, Started::Starting | Started::AlreadyRunning(_) | Started::Running(_)), "{second:?}");
    assert_eq!(starts(), before + 1, "one Atlas, not two");
    // Polled, as the apps do: the address arrives by itself.
    let until = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let url = loop {
        if let Phase::Running(u) = phase() {
            break u;
        }
        assert!(std::time::Instant::now() < until, "the hub never answered: {:?}", phase());
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    assert!(url.starts_with("http://127.0.0.1:"), "{url}");
    assert_eq!(start_once(h.clone(), 0, std::time::Duration::ZERO), Started::AlreadyRunning(url.clone()));
    assert_eq!(atlas::mobile::atlas_mobile_state(), 2);
    let mut buf = [0 as std::os::raw::c_char; 512];
    assert!(unsafe { atlas::mobile::atlas_mobile_url(buf.as_mut_ptr(), buf.len()) } > 0);
    assert_eq!(starts(), before + 1);
    // Stopped, it says so, and a start after that is a real new start.
    atlas::mobile::stop_now();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while phase() != Phase::Stopped {
        assert!(std::time::Instant::now() < until, "it never stopped: {:?}", phase());
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(unsafe { atlas::mobile::atlas_mobile_url(buf.as_mut_ptr(), buf.len()) }, -1, "no address once stopped");
}

#[test]
fn the_phone_apps_call_one_c_door_and_the_sources_are_in_the_tree() {
    let h = std::fs::read_to_string("mobile/atlas.h").unwrap();
    // atlas_mobile_state added 28 Sep 2026: the apps poll it (and the
    // address) instead of blocking their main thread on the start.
    for f in ["atlas_mobile_start", "atlas_mobile_state", "atlas_mobile_url", "atlas_mobile_network", "atlas_mobile_stop"] {
        assert!(h.contains(f), "atlas.h declares {f}");
    }
    let m = std::fs::read_to_string("src/mobile.rs").unwrap();
    assert_eq!(m.matches("#[no_mangle]").count(), 5, "and mobile.rs exports exactly those five");
    for f in [
        "mobile/ios/Atlas/AtlasApp.swift",
        "mobile/ios/Atlas/Shell.swift",
        "mobile/ios/Atlas/LiveActivity.swift",
        "mobile/ios/AtlasShare/ShareViewController.swift",
        "mobile/ios/AtlasLive/AtlasWidget.swift",
        "mobile/android/app/src/main/java/app/atlas/MainActivity.kt",
        "mobile/android/app/src/main/java/app/atlas/AtlasService.kt",
        "mobile/android/app/src/main/cpp/atlas_jni.c",
        "mobile/android/app/src/main/AndroidManifest.xml",
        "mobile/README.md",
        // Built without a Mac (26 Sep): the Xcode project as text, the cloud
        // builds, and signing on Eric's machine.
        "mobile/ios/project.yml",
        "mobile/build-android.sh",
        "mobile/sign-android.sh",
        "../.github/workflows/ios.yml",
        "../.github/workflows/android.yml",
    ] {
        assert!(std::path::Path::new(f).is_file(), "{f}");
    }
    // Speech stays on the phone.
    assert!(std::fs::read_to_string("mobile/ios/Atlas/Shell.swift").unwrap().contains("requiresOnDeviceRecognition = true"));
    assert!(std::fs::read_to_string("mobile/android/app/src/main/java/app/atlas/MainActivity.kt").unwrap().contains("EXTRA_PREFER_OFFLINE"));
    // Neither app locks the orientation (WCAG 1.3.4).
    assert!(!std::fs::read_to_string("mobile/android/app/src/main/AndroidManifest.xml").unwrap().contains("screenOrientation"));
    assert!(std::fs::read_to_string("mobile/ios/Atlas/Info.plist").unwrap().contains("UIInterfaceOrientationLandscapeLeft"));
    // The iPhone build signs with Apple's automatic signing and an API key:
    // no signing key is ever stored in the repository or the workflow.
    let ios = std::fs::read_to_string("../.github/workflows/ios.yml").unwrap();
    assert!(ios.contains("-allowProvisioningUpdates") && ios.contains("secrets.ASC_KEY_P8"));
    assert!(!ios.contains("BEGIN PRIVATE KEY"), "a key pasted into the workflow");
    // Android is signed on Eric's machine; the cloud build is unsigned, and the
    // signing script refuses to replace an existing key.
    let andr = std::fs::read_to_string("../.github/workflows/android.yml").unwrap();
    assert!(!andr.contains("${{ secrets."), "the Android build needs no secrets");
    let sign = std::fs::read_to_string("mobile/sign-android.sh").unwrap();
    assert!(sign.contains("Refusing to replace it"));
    // The app identity is the one Eric chose, the same on both phones.
    assert!(std::fs::read_to_string("mobile/ios/project.yml").unwrap().contains("PRODUCT_BUNDLE_IDENTIFIER: com.ericsnider.atlas\n"));
    assert!(std::fs::read_to_string("mobile/android/app/build.gradle.kts").unwrap().contains("applicationId = \"com.ericsnider.atlas\""));
    // Keys can't be committed by accident.
    let ignore = std::fs::read_to_string("../.gitignore").unwrap();
    for k in ["*.p12", "*.p8", "*.keystore", "*.pass"] {
        assert!(ignore.contains(k), "{k} isn't ignored");
    }
    // Plain http only to Atlas on the phone itself.
    let net = std::fs::read_to_string("mobile/android/app/src/main/res/xml/loopback_only.xml").unwrap();
    assert!(net.contains("cleartextTrafficPermitted=\"false\"") && net.contains("127.0.0.1"));
}

// ---------------------------------------------------------------- every page

fn every_page() -> Vec<(String, Action)> {
    let mut v: Vec<(String, Action)> = Vec::new();
    for (_, ps) in NAV {
        for p in *ps {
            v.push((p.href().to_string(), Action::Hub(*p)));
        }
    }
    for q in ["view=board", "view=calendar", "view=table"] {
        v.push((format!("/hub/tasks?{q}"), Action::HubQ(Page::SharedTasks, q.into())));
    }
    v
}

#[test]
fn every_page_is_built_for_everyone_on_every_screen() {
    let pages = with_daemon(|d| every_page().into_iter().map(|(n, a)| (n, get(d, a))).collect::<Vec<_>>());
    assert!(pages.len() >= 30, "the whole hub, not a sample: {}", pages.len());
    for (name, r) in &pages {
        assert_eq!(r.status, 200, "{name}");
        let h = &r.body;
        let ctx = |what: &str| format!("{name}: {what}");
        assert!(h.starts_with("<!doctype html><html lang=en"), "{}", ctx("a language (3.1.1)"));
        assert!(h.contains("<title>Atlas — "), "{}", ctx("a title (2.4.2)"));
        assert!(h.contains("<a class=skip href='#main'>"), "{}", ctx("a skip link (2.4.1)"));
        assert_eq!(h.matches("<main id=main").count(), 1, "{}", ctx("one main landmark"));
        assert!(h.contains("<nav class=sidebar aria-label='Atlas'>"), "{}", ctx("a named nav"));
        assert!(h.contains("<nav class=tabs aria-label='Main'>"), "{}", ctx("the phone's tab bar"));
        assert!(h.contains("<a class='tool help' href='/hub/help' aria-label='Help'>"), "{}", ctx("help in the same place on every page (3.2.6)"));
        assert!(!h.contains("http-equiv=refresh") && !h.contains("http-equiv='refresh'"), "{}", ctx("no timed refresh (2.2.1, 2.2.4)"));
        assert!(!h.contains("user-scalable=no") && !h.contains("maximum-scale"), "{}", ctx("pinch zoom allowed (1.4.4)"));
        assert!(h.contains("<h1"), "{}", ctx("a heading"));
        labelled(name, h);
    }
}

/// Every visible input, select and textarea has a name a screen reader can
/// say: a `<label for>`, an `aria-label`, or a wrapping label.
fn labelled(name: &str, h: &str) {
    let mut at = 0;
    while let Some(i) = h[at..].find(|c| c == '<').map(|i| i + at) {
        at = i + 1;
        let tag = &h[i..];
        let kind = ["<input", "<select", "<textarea"].into_iter().find(|t| tag.starts_with(t) && tag[t.len()..].starts_with([' ', '>']));
        let Some(_) = kind else { continue };
        let end = tag.find('>').unwrap_or(tag.len());
        let el = &tag[..end];
        if el.contains("type=hidden") || el.contains("type='hidden'") || el.contains("type=submit") {
            continue;
        }
        if el.contains("aria-label") || el.contains("aria-labelledby") {
            continue;
        }
        let id = el.split(" id=").nth(1).map(|s| s.trim_matches('\'').split([' ', '\'', '>']).next().unwrap_or("").to_string());
        let by_for = id.as_ref().is_some_and(|id| !id.is_empty() && (h.contains(&format!("for={id}")) || h.contains(&format!("for='{id}'"))));
        let wrapped = h[..i].rfind("<label").is_some_and(|l| !h[l..i].contains("</label>"));
        assert!(by_for || wrapped, "{name}: a control with no label (1.3.1, 4.1.2): {el}");
    }
}

#[test]
fn the_new_pages_are_the_design_not_placeholders() {
    with_daemon(|d| {
        let now = atlas::store::now();
        let mut tasks = atlas::shared_task::Tasks::load(&d.store);
        tasks.add(atlas::earned::Space::Business("Northwind LLC".into()), "Send Q3 invoice to Meridian", Some(now + 2 * 86400), now);
        tasks.save(&d.store).unwrap();
        let mut cl = atlas::clients::ClientList::load(&d.store);
        cl.add("maya@riverstone.example", "Maya Riverstone", "Renewing in March", now);
        cl.save(&d.store).unwrap();

        let board = get(d, Action::HubQ(Page::SharedTasks, "view=board".into())).body;
        assert!(board.contains("Send Q3 invoice to Meridian"), "the task is on the board");
        let cal = get(d, Action::HubQ(Page::SharedTasks, "view=calendar".into())).body;
        assert!(cal.contains("Send Q3 invoice to Meridian"), "and on the calendar");
        for (p, words) in [
            (Page::Business, "Northwind LLC"),
            (Page::Clients, "Maya Riverstone"),
            (Page::Messages, "Messages"),
            (Page::Documents, "Documents"),
            (Page::Sound, "Quiet hours"),
            (Page::Trusted, "Trusted"),
            (Page::Partners, "Partners"),
        ] {
            let h = get(d, Action::Hub(p)).body;
            assert!(h.contains(words), "{}: {words}", p.href());
        }
    });
}

// ---------------------------------------------------------------- the forms

fn location(r: &Reply) -> &str {
    assert_eq!(r.status, 303, "a form answers with a redirect: {}", r.body);
    &r.body
}

#[test]
fn a_share_from_the_phone_lands_on_give_and_in_the_tray() {
    with_daemon(|d| {
        let before = d.tray.items.len();
        let r = post(d, "/hub/give", &[("text", "https://example.com/q3-report")]);
        assert!(location(&r).starts_with("/hub/give"), "back to Give, with what was said");
        assert_eq!(d.tray.items.len(), before + 1, "handed to the tray");
        // A share target arrives as a GET with ?title&text&url.
        // Taken once, then on to the page's own address (a redirect), so a
        // reload doesn't add it again (28 Sep 2026: this asserted a 200 the
        // hub stopped giving when that redirect was added).
        let r = get(d, Action::HubQ(Page::Give, "title=Report&url=https%3A%2F%2Fexample.com%2Fshared".into()));
        assert_eq!(r.status, 303);
        assert_eq!(d.tray.items.len(), before + 2, "the shared link is in the tray too");
        let back = location(&r);
        assert!(back.starts_with("/hub/give?"), "{back}");
        let page = get(d, Action::HubQ(Page::Give, back.trim_start_matches("/hub/give?").to_string()));
        assert_eq!(d.tray.items.len(), before + 2, "following the redirect adds nothing");
        assert!(page.body.contains("role=status") || page.body.contains("aria-live"), "and the page says so, to a screen reader too");
    });
}

/// 28 Sep 2026: the Android app's `atlas://` link is open to every app on the
/// phone, and it (and a share) used to load `/hub/give?text=...`, which hands
/// the text to Atlas at once. The apps now open Give with `draft=`: the words
/// are shown in the box, and nothing is handed over until you press the button.
#[test]
fn a_link_from_another_app_only_fills_the_box_it_never_hands_anything_over() {
    with_daemon(|d| {
        let before = d.tray.items.len();
        let page = get(d, Action::HubQ(Page::Give, "draft=%3Cb%3Eread+this%3C%2Fb%3E".into()));
        assert_eq!(page.status, 200);
        assert_eq!(d.tray.items.len(), before, "a draft is only shown");
        assert!(page.body.contains("&lt;b&gt;read this&lt;/b&gt;</textarea>"), "in the box, escaped");
        assert!(!page.body.contains("<b>read this</b>"));
    });
}

#[test]
fn the_phone_apps_links_only_navigate() {
    // The shells turn a link's words into a draft and drop every other part
    // of its query, so a link can open a page but never change anything.
    let kt = std::fs::read_to_string("mobile/android/app/src/main/java/app/atlas/MainActivity.kt").unwrap();
    assert!(kt.contains("draft="), "Android's share and links open Give with a draft");
    assert!(!kt.contains("\"/hub/give?text=\""), "Android still hands shared text straight over");
    let swift = std::fs::read_to_string("mobile/ios/Atlas/AtlasApp.swift").unwrap();
    assert!(swift.contains("draft"), "the iPhone's links open Give with a draft");
    assert!(!swift.contains("link.query.map"), "the iPhone still passes a link's whole query to the hub");
}

#[test]
fn a_spoken_turn_is_read_back_only_when_sound_and_voice_allows() {
    with_daemon(|d| {
        // Typed: never marked to be read out.
        let typed = post(d, "/hub/talk", &[("text", "what time is it")]);
        assert_eq!(location(&typed), "/hub/talk");
        // Muted: a spoken turn is answered on screen only.
        let said = d_set(d, "muted", "on");
        assert!(!said.is_empty());
        let muted = post(d, "/hub/talk", &[("text", "what time is it"), ("spoken", "1")]);
        assert_eq!(location(&muted), "/hub/talk", "muted means not said");
        // Unmuted, hands-free: a spoken turn is read back.
        d_set(d, "muted", "off");
        d_set(d, "speak_replies", "hands_free");
        let spoken = post(d, "/hub/talk", &[("text", "what time is it"), ("spoken", "1")]);
        assert_eq!(location(&spoken), "/hub/talk?say=1");
        // The page reads it out only through the phone app's own voice.
        let page = get(d, Action::Hub(Page::Talk)).body;
        assert!(page.contains("window.atlasHeard") && page.contains("s.speak"), "the shell hooks are on the page");
        assert!(page.contains("name=spoken value=0"), "typed by default");
    });
}

fn d_set(d: &mut Daemon, key: &str, value: &str) -> String {
    let r = post(d, "/hub/sound", &[("key", key), ("value", value)]);
    location(&r).to_string()
}

#[test]
fn sound_settings_are_kept_and_nonsense_is_refused() {
    with_daemon(|d| {
        let ok = post(d, "/hub/sound", &[("key", "quiet_hours"), ("from", "22:00"), ("to", "07:00")]);
        assert!(location(&ok).starts_with("/hub/sound"));
        assert_eq!(d.tools_cfg().sound.quiet_from, "22:00");
        let bad = post(d, "/hub/sound", &[("key", "quiet_hours"), ("from", "late"), ("to", "07:00")]);
        assert!(location(&bad).contains("didn"), "said, not swallowed: {}", location(&bad));
        let vol = post(d, "/hub/sound", &[("key", "volume"), ("value", "40")]);
        location(&vol);
        assert_eq!(d.tools_cfg().sound.volume, 40);
    });
}

// ---------------------------------------------------------------- live and installed

#[test]
fn live_json_is_what_the_live_activity_reads() {
    with_daemon(|d| {
        let r = get(d, Action::LiveJson);
        let v: serde_json::Value = serde_json::from_str(&r.body).expect("json");
        for k in ["status", "working", "ready", "waiting", "brief"] {
            assert!(v.get(k).is_some(), "live.json has {k}");
        }
        assert!(v["ready"].is_array() && v["waiting"].is_u64());
    });
}

#[test]
fn the_installed_app_takes_shares_and_turns_with_the_phone() {
    let m: serde_json::Value = serde_json::from_str(&atlas::hub::manifest("tok")).unwrap();
    assert_eq!(m["share_target"]["action"], "/hub/give");
    assert_eq!(m["share_target"]["method"], "GET");
    assert_eq!(m["share_target"]["params"]["url"], "url");
    assert!(m.get("orientation").is_none(), "no orientation lock (1.3.4)");
}

// ---------------------------------------------------------------- the native windows

#[cfg(feature = "desktop-ui")]
#[test]
fn native_windows_follow_this_computers_settings() {
    use atlas::appearance::{Appearance, Theme};
    use atlas::look_paint::palette::{for_appearance_on, from_contrast, EMBER_DARK, WARM_PAPER};
    use atlas::oslook::{Contrast, OsLook};
    let system = Appearance { theme: Theme::System, ..Default::default() };
    let dark = OsLook { dark: Some(true), ..Default::default() };
    let light = OsLook { dark: Some(false), ..Default::default() };
    assert_eq!(for_appearance_on(&system, &dark), EMBER_DARK, "Follow this computer + dark apps");
    assert_eq!(for_appearance_on(&system, &light), WARM_PAPER);
    assert_eq!(for_appearance_on(&system, &OsLook::default()), WARM_PAPER, "unreadable: the lead colourway");
    let warm = Appearance { theme: Theme::Warm, ..Default::default() };
    assert_eq!(for_appearance_on(&warm, &dark), WARM_PAPER, "a chosen colourway stays chosen");
    // Windows high contrast wins, in the user's own colours.
    let c = Contrast { window: 0x000000, text: 0xFFFFFF, highlight: 0x1AEBFF, gray: 0x3FF23F, link: 0xFFFF00 };
    let hc = OsLook { contrast: Some(c), ..Default::default() };
    let p = for_appearance_on(&warm, &hc);
    assert_eq!(p, from_contrast(&c));
    assert!(p.dark && p.ink == eframe_rgb(0x000000) && p.text == eframe_rgb(0xFFFFFF));
    // Every window dresses for the frame, which carries text size and motion.
    for f in ["src/window.rs", "src/setupwin.rs", "src/overlaywin.rs", "src/typebox.rs"] {
        assert!(std::fs::read_to_string(f).unwrap().contains("crate::look_paint::dress(ctx);"), "{f}");
    }
    // And the screen reader bridge is compiled in.
    assert!(std::fs::read_to_string("Cargo.toml").unwrap().contains("\"accesskit\""));
}

#[cfg(feature = "desktop-ui")]
fn eframe_rgb(v: u32) -> eframe::egui::Color32 {
    eframe::egui::Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

// ---------------------------------------------------------------- the rules underneath

#[test]
fn quiet_hours_and_windows_settings_are_read_the_way_they_are_written() {
    // Quiet hours cross midnight; Windows writes text size as a percentage
    // and colours as 0x00BBGGRR. Each read as written, or the setting means
    // something other than what you chose.
    let s = atlas::sound::SoundConfig { quiet_hours: true, ..Default::default() };
    assert!(s.in_quiet_hours(23 * 60) && s.in_quiet_hours(60) && !s.in_quiet_hours(12 * 60));
    assert_eq!(atlas::oslook::text_scale_from_percent(125), 1.25);
    assert_eq!(atlas::oslook::colorref_to_rgb(0x0000_00FF), 0xFF_0000);
}

/// Sending on the Talk page never waits for the answer (27 Sep 2026: it hung
/// while the model thought, and so did the rest of the hub). The page shows
/// the words and "thinking…" at once, and the reply on the next tick.
#[test]
fn talk_comes_straight_back_and_the_reply_lands_on_the_next_tick() {
    with_daemon(|d| {
        let r = post(d, "/hub/talk", &[("text", "what time is it")]);
        assert_eq!(location(&r), "/hub/talk");
        let page = get(d, Action::Hub(Page::Talk)).body;
        assert!(page.contains("what time is it") && page.contains("thinking…"), "{page}");
        assert!(page.contains("location.reload()"), "the page doesn't look again");
        let _ = d.tick(atlas::store::now());
        let page = get(d, Action::Hub(Page::Talk)).body;
        assert!(!page.contains("thinking…"), "still thinking after its turn");
        assert!(page.contains("It&#39;s") || page.contains("It's"), "no reply on the page: {page}");
    });
}
