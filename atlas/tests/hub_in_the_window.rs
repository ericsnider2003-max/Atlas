//! The hub inside Atlas's own window, and Windows' blocks answered without a
//! certificate (23 Sep 2026).
//!
//! Eric: *"What was the hub designed for … if it's not going to get used or
//! accessible on the laptop? … that's how everything can be tracked, things
//! get accessed, and settings changed."* His ruling: the hub shows inside
//! Atlas's window, drawn by Windows' own web view — not a browser, and not a
//! dependency on the internet. These tests hold the second half of that: the
//! region only ever loads Atlas's own hub on this machine.

use atlas::firstlaunch::{app_control_from, app_control_words, download_mark_of, AppControl};
use atlas::hubwin::{stays_on_the_hub, page_url, Area};
use atlas::setupwin::First;
use std::path::Path;

#[test]
fn the_hub_region_only_goes_to_atlas_own_hub_on_this_machine() {
    let port = 8787;
    for ok in [
        "http://127.0.0.1:8787/hub?t=abc",
        "http://127.0.0.1:8787/hub/outstanding",
        "http://localhost:8787/hub/settings",
        "http://127.0.0.1:8787",
        "about:blank",
    ] {
        assert!(stays_on_the_hub(ok, port), "refused its own hub: {ok}");
    }
    for no in [
        "https://www.google.com/",
        "http://example.com/hub",
        "http://127.0.0.1:8788/hub",
        "http://127.0.0.1:87870/hub",
        "http://127.0.0.1:8787.evil.example/hub",
        "http://127.0.0.1:8787@evil.example/hub",
        "https://127.0.0.1:8787/hub",
        "file:///C:/Users/erics/Documents/secret.txt",
        "javascript:alert(1)",
        "",
    ] {
        assert!(!stays_on_the_hub(no, port), "the hub region would have gone to {no:?} — that makes it a browser");
    }
}

#[test]
fn a_link_out_of_the_hub_goes_to_your_browser_and_nothing_else_does() {
    use atlas::hubwin::for_the_browser;
    for yes in ["https://accounts.google.com/security", "http://example.com/x?y=1"] {
        assert!(for_the_browser(yes, 8787), "{yes}");
    }
    for no in [
        "http://127.0.0.1:8787/hub/settings",
        "file:///C:/Windows/System32/calc.exe",
        "javascript:alert(1)",
        "ms-settings:privacy",
        "https://example.com/a b",
        "https://example.com/\"&calc",
        "",
    ] {
        assert!(!for_the_browser(no, 8787), "{no} would be handed to the shell");
    }
}

#[test]
fn a_hub_page_address_is_made_the_one_way_hub_addresses_are_made() {
    assert_eq!(page_url(8787, "tok", "/hub/outstanding"), atlas::server::hub_url(8787, "tok", "/hub/outstanding"));
    // Anything that isn't a hub page opens the hub's front page instead.
    assert_eq!(page_url(8787, "tok", "https://elsewhere.example"), atlas::server::hub_url(8787, "tok", "/hub"));
    assert_eq!(page_url(8787, "tok", "/hub?x=1"), atlas::server::hub_url(8787, "tok", "/hub"));
    assert!(stays_on_the_hub(&page_url(9000, "tok", "/hub/status"), 9000));
}

#[test]
fn the_window_opens_on_the_page_it_was_asked_for_and_says_so_the_same_way() {
    let w = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    assert_eq!(First::from_words(&w(&[])), First::Home);
    assert_eq!(First::from_words(&w(&["settings"])), First::Settings);
    assert_eq!(First::from_words(&w(&["hub"])), First::Hub("/hub".into()));
    assert_eq!(First::from_words(&w(&["hub", "outstanding"])), First::Hub("/hub/outstanding".into()));
    assert_eq!(First::from_words(&w(&["Hub", "/hub/Status/"])), First::Hub("/hub/status".into()));
    // A page name that isn't a plain word can't smuggle an address in.
    assert_eq!(First::from_words(&w(&["hub", "../../etc"])), First::Hub("/hub".into()));
    assert_eq!(First::from_words(&w(&["hub", "x?t=stolen"])), First::Hub("/hub".into()));
    // Moving into its home re-opens on the same page: the words round-trip.
    for f in [First::Home, First::Settings, First::Hub("/hub".into()), First::Hub("/hub/activity".into())] {
        let words = f.words();
        assert_eq!(words[0], "home");
        assert_eq!(First::from_words(&words[1..]), f, "{words:?}");
    }
}

#[test]
fn the_hub_region_moves_only_when_it_has_really_moved() {
    let a = Area { x: 28.0, y: 60.0, w: 500.0, h: 700.0 };
    assert!(a.same_as(&Area { x: 28.2, y: 60.0, w: 500.0, h: 700.3 }));
    assert!(!a.same_as(&Area { x: 28.0, y: 60.0, w: 520.0, h: 700.0 }));
}

#[test]
fn the_hub_region_is_not_another_program_and_is_not_on_linux_builds() {
    let src = std::fs::read_to_string("src/hubwin.rs").unwrap();
    let code: String = src.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
    assert!(!code.contains("Command::new"), "the hub region launches a program — that's a browser by another name");
    // 27 Sep 2026: turned away from the region, a web address now opens in
    // your own browser rather than doing nothing; the region itself still
    // never leaves the hub.
    assert!(code.contains("with_navigation_handler(move |u| stays_on_the_hub(&u, port) || turn_away(&log_dir, &u, port))"), "navigation isn't held to `stays_on_the_hub`");
    assert!(code.contains("ShellExecuteW("), "links out of the hub don't reach the browser");
    assert!(code.contains(".with_focused(false)"), "taking focus at creation throws the hub away when the window is behind another");
    assert!(code.contains("NewWindowResponse::Deny"), "a page could open a window of its own");
    // The web view is Windows' own, and only Windows builds carry it.
    let cargo = std::fs::read_to_string("Cargo.toml").unwrap();
    let (before, after) = cargo.split_once("[target.'cfg(windows)'.dependencies]").expect("a Windows-only section");
    assert!(!before.contains("\nwry"), "wry is a dependency on every platform");
    let line = after.lines().find(|l| l.starts_with("wry")).expect("wry is a Windows-only dependency");
    assert!(line.contains("os-webview") && line.contains("default-features = false"), "{line}");
}

#[test]
fn smart_app_control_is_read_from_what_windows_says() {
    let said = |v: &str| {
        format!(
            "\r\nHKEY_LOCAL_MACHINE\\SYSTEM\\CurrentControlSet\\Control\\CI\\Policy\r\n    VerifiedAndReputablePolicyState    REG_DWORD    {v}\r\n\r\n"
        )
    };
    assert_eq!(app_control_from(&said("0x0")), AppControl::Off);
    assert_eq!(app_control_from(&said("0x1")), AppControl::On);
    assert_eq!(app_control_from(&said("0x2")), AppControl::Evaluating);
    assert_eq!(app_control_from(&said("0x7")), AppControl::Unknown);
    assert_eq!(app_control_from("ERROR: The system was unable to find the specified registry key or value."), AppControl::Unknown);
    assert_eq!(app_control_from(""), AppControl::Unknown);
}

#[test]
fn smart_app_control_is_mentioned_only_when_it_can_block_atlas_and_says_where_the_switch_is() {
    assert_eq!(app_control_words(AppControl::Off), None);
    assert_eq!(app_control_words(AppControl::Unknown), None);
    for s in [AppControl::On, AppControl::Evaluating] {
        let w = app_control_words(s).unwrap();
        assert!(w.contains("Windows Security → App & browser control → Smart App Control settings"), "{w}");
        assert!(w.contains("back on"), "doesn't say it can be undone: {w}");
    }
    assert_ne!(app_control_words(AppControl::On), app_control_words(AppControl::Evaluating));
}

#[test]
fn the_download_mark_is_the_second_stream_windows_keeps_beside_a_file() {
    let m = download_mark_of(Path::new(r"C:\Users\erics\AppData\Local\Atlas\atlas.exe"));
    assert_eq!(m.to_string_lossy(), r"C:\Users\erics\AppData\Local\Atlas\atlas.exe:Zone.Identifier");
    if !cfg!(windows) {
        // Nothing to take off where there are no download marks.
        let f = std::env::temp_dir().join("atlas-no-mark.txt");
        std::fs::write(&f, b"x").unwrap();
        assert!(!atlas::firstlaunch::forget_download_mark(&f));
        assert!(f.exists(), "taking off a mark removed the file itself");
    }
    // Moving in takes the mark off the copy, so Windows asks once, at the
    // download, and not at every start.
    let src = std::fs::read_to_string("src/firstlaunch.rs").unwrap();
    // (28 Sep 2026: `move_in` became `move_in_over`, which also asks a
    // running Atlas to stop first.)
    let over = src.split_once("pub fn move_in_over(").unwrap().1.split("\npub fn ").next().unwrap();
    assert!(over.contains("forget_download_mark(&target)"));
}

#[test]
fn a_hidden_part_of_a_hub_page_stays_hidden() {
    // Seen on the real laptop the first time the hub showed in Atlas's
    // window: the "What do you want to do?" palette was open over the page on
    // arrival. `.palette{display:flex}` outranked the browser's own rule for
    // the `hidden` attribute, so it never hid — on the phone too.
    let page = atlas::hub::shell("Test", "<p>body</p>");
    assert!(page.contains("[hidden]{display:none!important}"), "`hidden` can be overruled by a display rule");
    let palette_at = page.find(".palette{").map(|i| i as i64).unwrap_or(-1);
    let hidden_at = page.find("[hidden]{display:none!important}").map(|i| i as i64).unwrap_or(-1);
    assert!(hidden_at > 0 && (palette_at < 0 || hidden_at > palette_at), "the `hidden` rule must come after the palette's own");
}

#[test]
fn a_hub_address_written_down_never_carries_the_token() {
    use atlas::hubwin::without_token;
    assert_eq!(without_token("http://127.0.0.1:8787/hub?t=secret"), "http://127.0.0.1:8787/hub?t=…");
    assert_eq!(without_token("http://127.0.0.1:8787/hub?t=secret&x=1"), "http://127.0.0.1:8787/hub?t=…&x=1");
    assert_eq!(without_token("http://127.0.0.1:8787/hub/status"), "http://127.0.0.1:8787/hub/status");
    assert!(!without_token(&page_url(8787, "abc123", "/hub/outstanding")).contains("abc123"));
}
