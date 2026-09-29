//! The hub reaches the internet for nothing.
//!
//! Atlas runs on your machine and its hub has to work with the network pulled
//! — the same page, whole, on a plane or in a blackout. That rules out the
//! things a web dashboard reaches for by habit: a font from a CDN, an icon
//! pack, a script from unpkg, a stylesheet from a design system's servers.
//! Every one of those is a page that renders wrong exactly when you can't get
//! online to notice.
//!
//! So the whole hub is inline: system fonts, hand-drawn SVG, CSS and JS in the
//! page. This guard renders the real pages and fails the moment anything in
//! them points off the machine. It is the test that keeps "works offline" true
//! rather than a thing that was true once.

use atlas::dash::Layout;
use atlas::hub::{self, Page};

/// The one off-machine string that is allowed, because it is never fetched:
/// the SVG XML namespace. `xmlns="http://www.w3.org/2000/svg"` is an
/// identifier the renderer matches literally; it opens no connection.
const ALLOWED: &[&str] = &["http://www.w3.org/"];

/// Every off-machine reference in the html, with the allowed namespace removed.
fn network_refs(html: &str) -> Vec<String> {
    let mut hits = Vec::new();
    for (i, _) in html.match_indices("http") {
        // Only http:// or https://, so "throughput" and the like don't match.
        let rest = &html[i..];
        if !rest.starts_with("http://") && !rest.starts_with("https://") {
            continue;
        }
        if ALLOWED.iter().any(|a| rest.starts_with(a)) {
            continue;
        }
        // Grab the URL up to the first quote, space or bracket, for the message.
        let end = rest.find(['"', '\'', ' ', ')', '>', '<']).unwrap_or(rest.len().min(60));
        hits.push(rest[..end].to_string());
    }
    // The other ways a page pulls something in, none of which the hub uses.
    for needle in ["cdn", "unpkg", "jsdelivr", "googleapis", "gstatic", "@import", "//fonts."] {
        if html.contains(needle) {
            hits.push(format!("uses '{needle}'"));
        }
    }
    hits
}

fn every_page() -> Vec<(&'static str, String)> {
    let mut out = vec![
        ("dashboard", hub::dashboard_page(&Layout::default(), &[], false)),
        ("dashboard-arranging", hub::dashboard_page(&Layout::default(), &[], true)),
        ("shell", hub::shell_at(Some(Page::Settings), "Settings", "<p>body</p>")),
    ];
    // The shell is what carries the fonts, styles and scripts, so a page
    // rendered through it is the real test. One is enough to cover the frame,
    // but the dashboard is where a redesign is most likely to reach for a
    // charting library or an icon font, so it is checked on its own too.
    out.push(("plain-shell", hub::shell("Atlas", "<p>hello</p>")));
    out
}

#[test]
fn no_page_in_the_hub_reaches_off_the_machine() {
    for (name, html) in every_page() {
        let refs = network_refs(&html);
        assert!(
            refs.is_empty(),
            "the {name} page reaches the internet, so it won't render whole offline:\n  {}",
            refs.join("\n  ")
        );
    }
}

#[test]
fn the_fonts_are_the_ones_already_on_the_machine() {
    // A web font is the commonest offline break: the page loads, the text is
    // invisible or wrong until the font arrives, and it never arrives. The hub
    // asks only for the system stack.
    let html = hub::shell("Atlas", "<p>x</p>");
    assert!(html.contains("system-ui"), "the system font stack should be named");
    assert!(!html.contains("font-face"), "no bundled or fetched font faces");
    assert!(!html.contains("fonts.google"), "no Google Fonts");
}

#[test]
fn light_dark_and_contrast_follow_the_system_with_no_script() {
    // Appearance adapts to the system through media queries — no script, no
    // stored round-trip, no network. The hooks for a stored override are in the
    // CSS, applied server-side, so the manual switch stays script-free too.
    let html = hub::shell_at(Some(Page::Dashboard), "Dashboard", "<p>x</p>");
    assert!(!html.to_lowercase().contains("<script"), "appearance needs no script");
    assert!(html.contains("prefers-color-scheme"), "it follows the system light/dark");
    assert!(html.contains("prefers-contrast"), "and the system's contrast preference");
    assert!(network_refs(&html).is_empty(), "and pulls nothing in");
}
