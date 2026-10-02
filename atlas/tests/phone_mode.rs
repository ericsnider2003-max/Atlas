//! The phone app shows what works on a phone, and nothing that only works
//! on a laptop or that Apple's review refuses (`phonemode`, 2 Oct 2026).

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::hub::{self, Page};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::server::Action;
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-phonemode-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

const LAPTOP_ONLY: [Page; 6] = [Page::Phone, Page::Updates, Page::Sync, Page::Offline, Page::Gestures, Page::AddOns];

#[test]
fn the_laptop_only_pages_are_not_there_on_the_phone() {
    atlas::phonemode::on_this_thread_for_test(true);
    for p in LAPTOP_ONLY {
        assert_eq!(hub::route(p.href()), None, "{} opens on the phone", p.label());
    }
    assert_eq!(hub::route("/hub"), Some(Page::Dashboard));
    assert_eq!(hub::route("/hub/settings"), Some(Page::Settings));
    // The same Atlas on a laptop still has every one of them.
    atlas::phonemode::on_this_thread_for_test(false);
    for p in LAPTOP_ONLY {
        assert_eq!(hub::route(p.href()), Some(p), "{} is gone from the laptop", p.label());
    }
}

#[test]
fn home_on_the_phone_links_to_nothing_a_phone_cannot_do() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("home")), Proactive::new(ProactiveConfig::default()));
    atlas::phonemode::on_this_thread_for_test(true);
    let home = atlas::hublive::reply(&mut d, Action::Hub(Page::Dashboard)).body;
    for gone in LAPTOP_ONLY {
        let at = home.find(&format!("href='{}'", gone.href()));
        assert_eq!(at, None, "home links to {}: {}", gone.label(), at.map(|i| &home[i.saturating_sub(300)..(i + 100).min(home.len())]).unwrap_or(""));
    }
    // What's on screen: the stylesheet's own comments aren't.
    let shown = match (home.find("<style>"), home.find("</style>")) {
        (Some(a), Some(b)) => format!("{}{}", &home[..a], &home[b..]),
        _ => home.clone(),
    };
    for laptop_words in ["Tailscale", "Add your phone", "Ctrl K", "Organize my PC", "has no page yet"] {
        assert_eq!(shown.matches(laptop_words).count(), 0, "home on the phone says {laptop_words:?}");
    }
    assert!(home.contains("AtlasShell.calendar()"), "the calendar is asked for from a tap");
    let status = atlas::hublive::reply(&mut d, Action::Hub(Page::Status)).body;
    assert_eq!(status.matches("starts with Windows").count(), 0, "the status page talks about Windows");
    atlas::phonemode::on_this_thread_for_test(false);
}

#[test]
fn a_phone_without_a_model_says_how_to_get_one() {
    let said = atlas::daemon::model_failed_words(
        "Model unreachable: platform: there's no language model on this phone yet -- say \"get your own model\" (best on wifi)",
    );
    assert_eq!(said, atlas::phonemode::NO_MODEL_YET);
    // Anything else is still a failed answer, said as one.
    assert!(atlas::daemon::model_failed_words("timed out").starts_with("I couldn't get an answer"));
}

#[test]
fn the_phone_starts_with_no_trading_check_ins() {
    let mut t = Config::load(Path::new("config")).unwrap().tools.unwrap();
    atlas::phonemode::as_a_phone(&mut t);
    assert_eq!((t.workday.trade_day.enabled, t.workday.trade_day.prompt), (false, false));
    // The online models stay, behind the yes (the test below).
    assert!(t.models.online_second);
}

#[test]
fn the_online_models_wait_for_your_yes_on_the_phone() {
    use atlas::phonemode::{online_answer, ASK_ONLINE};
    assert_eq!(online_answer("use online models"), Some(true));
    assert_eq!(online_answer("Yes, use online models."), Some(true));
    assert_eq!(online_answer("stop using online models"), Some(false));
    assert_eq!(online_answer("what models do you use"), None);
    // Before the yes, the free online models put the question to you.
    atlas::phonemode::on_this_thread_for_test(true);
    atlas::phonemode::set_online_ok(false);
    let got = atlas::freeonline::FreeOnline::new().ask("system", "hello");
    atlas::phonemode::on_this_thread_for_test(false);
    let why = got.expect_err("nothing is sent before the yes").to_string();
    assert!(why.contains(ASK_ONLINE), "{why}");
    assert_eq!(atlas::daemon::model_failed_words(&format!("Model unreachable: platform: {why}")), ASK_ONLINE);
    let said = atlas::phonemode::models_said();
    assert!(said.ends_with("nothing goes to the online models."), "{said}");
    // One test, not two: the yes is the app's own, for every thread.
    a_yes_said_to_the_phone_is_kept();
    atlas::phonemode::set_online_ok(false);
}

fn a_yes_said_to_the_phone_is_kept() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let store = tmp("yes");
    let mut d = Daemon::new(&c, &p, None, Store::new(store.clone()), Proactive::new(ProactiveConfig::default()));
    atlas::phonemode::on_this_thread_for_test(true);
    let said = d.turn("use online models", 1_790_740_000);
    atlas::phonemode::on_this_thread_for_test(false);
    assert!(said.starts_with("Done: until this phone has a model of its own"), "{said}");
    assert!(Store::new(store).load::<bool>(atlas::phonemode::ONLINE_ASKED), "the yes wasn't kept");
    // On a laptop the same words aren't the phone's question.
    let laptop = d.turn("use online models", 1_790_740_100);
    assert!(!laptop.starts_with("Done: until this phone"), "{laptop}");
}
