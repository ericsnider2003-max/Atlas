//! The Hub holds its own house style (Plan E, 24 Oct 2026, Eric: "I am tired
//! of low quality results"). `taste::review` -- the offline design lint
//! Atlas already ran on pages it builds for you -- now also knows the tells
//! of generic, generated design, and every Hub page is held to it, in its
//! light and dark colours. The locked design is `design/hub/SPEC.md`; this
//! is the guard that it doesn't drift into a template.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::hub::Page;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::server::Action;
use atlas::store::Store;
use atlas::taste::{review, Rules, Severity};
use std::path::Path;

fn has(html: &str, rule: &str) -> bool {
    review(html, &Rules::default())
        .iter()
        .any(|f| f.rule == rule)
}

// ---- each tell is caught, and its absence is not mistaken for it ----

#[test]
fn a_purple_to_blue_gradient_is_caught() {
    let slop = r#"<html lang=en><style>.hero{background:linear-gradient(135deg,#7c3aed,#2563eb)}</style><p>x</p></html>"#;
    assert!(has(slop, "generic gradient"));
    let named = r#"<html lang=en><style>.hero{background:linear-gradient(to right, purple, blue)}</style></html>"#;
    assert!(has(named, "generic gradient"));
    let warm =
        r#"<html lang=en><style>.band{background:linear-gradient(#fdecc8,#ffffff)}</style></html>"#;
    assert!(
        !has(warm, "generic gradient"),
        "a warm, quiet gradient is not the template one"
    );
}

#[test]
fn a_card_inside_a_card_is_caught() {
    let slop =
        r#"<html lang=en><div class="card"><h2>A</h2><div class='card small'>B</div></div></html>"#;
    assert!(has(slop, "cards in cards"));
    let side_by_side = r#"<html lang=en><div class=cards><div class=card>A</div><div class=card>B</div></div></html>"#;
    assert!(
        !has(side_by_side, "cards in cards"),
        "a row of cards is not nesting"
    );
}

#[test]
fn text_too_faint_for_its_background_is_caught_in_either_theme() {
    let slop = r#"<html lang=en><style>:root{--ink:#9a9a9a;--bg:#ffffff}.note{color:var(--ink);background:var(--bg)}</style></html>"#;
    let f = review(slop, &Rules::default());
    let c = f.iter().find(|f| f.rule == "contrast").expect("contrast");
    assert_eq!(c.severity, Severity::Blocking);
    assert!(c.detail.contains("light"), "{}", c.detail);
    // Fine in light, too faint in dark.
    let dark_only = r#"<html lang=en><style>:root{--ink:#222222;--bg:#ffffff}:root[data-theme=dark]{--ink:#333333;--bg:#111111}.note{color:var(--ink);background:var(--bg)}</style></html>"#;
    let f = review(dark_only, &Rules::default());
    assert!(
        f.iter()
            .any(|f| f.rule == "contrast" && f.detail.contains("dark")),
        "{f:?}"
    );
    let fine = r#"<html lang=en><style>:root{--ink:#37352f;--bg:#ffffff}.note{color:var(--ink);background:var(--bg)}</style></html>"#;
    assert!(!has(fine, "contrast"));
}

#[test]
fn a_stock_web_typeface_first_is_advisory() {
    let slop = r#"<html lang=en><style>body{font-family:Inter, sans-serif}</style></html>"#;
    let f = review(slop, &Rules::default());
    let t = f
        .iter()
        .find(|f| f.rule == "stock typeface")
        .expect("typeface");
    assert_eq!(t.severity, Severity::Advisory);
    assert!(!has(
        r#"<html lang=en><style>body{font-family:system-ui,sans-serif}</style></html>"#,
        "stock typeface"
    ));
}

#[test]
fn the_slop_rules_can_be_turned_off_in_the_house_style() {
    let slop =
        r#"<html lang=en><style>.h{background:linear-gradient(#7c3aed,#2563eb)}</style></html>"#;
    let off = Rules {
        slop: false,
        ..Rules::default()
    };
    assert!(!review(slop, &off)
        .iter()
        .any(|f| f.rule == "generic gradient"));
}

// ---- the Hub itself ----

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }])
}

/// The page as a browser draws it: its linked stylesheet in place.
fn styled(page: String) -> String {
    let link = format!("<link rel=stylesheet href={}>", atlas::hub::style_path());
    page.replace(&link, &format!("<style>{}</style>", atlas::hub::STYLE))
}

#[test]
fn every_hub_page_is_free_of_the_tells_and_the_blocking_floors() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let root = std::env::temp_dir().join(format!("atlas-not-slop-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mut d = Daemon::new(
        &c,
        &p,
        None,
        Store::new(&root),
        Proactive::new(ProactiveConfig::default()),
    );
    let mut problems = Vec::new();
    let mut advisories = 0;
    for page in [
        Page::Dashboard,
        Page::Now,
        Page::Outstanding,
        Page::Talk,
        Page::Workshop,
        Page::Messages,
        Page::Settings,
    ] {
        let html = styled(atlas::hublive::reply(&mut d, Action::Hub(page)).body);
        for f in review(&html, &Rules::default()) {
            let slop = matches!(
                f.rule.as_str(),
                "generic gradient" | "cards in cards" | "contrast" | "stock typeface"
            );
            if f.severity == Severity::Blocking || slop {
                problems.push(format!("{}: {} -- {}", page.label(), f.rule, f.detail));
            } else {
                advisories += 1;
            }
        }
    }
    eprintln!(
        "Hub design review: {} problems, {advisories} advisories",
        problems.len()
    );
    assert!(
        problems.is_empty(),
        "the Hub drifted:\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn a_hidden_input_or_one_inside_its_label_is_named_a_bare_one_is_not() {
    let rules = Rules::default();
    let named = |html: &str| {
        !review(html, &rules)
            .iter()
            .any(|f| f.rule == "control name")
    };
    assert!(named(
        r#"<html lang=en><form><input type=hidden name=what value=x></form></html>"#
    ));
    assert!(named(
        r#"<html lang=en><label>Your name <input name=n></label></html>"#
    ));
    assert!(
        !named(r#"<html lang=en><form><input name=n></form></html>"#),
        "a bare input still needs a name"
    );
    assert!(
        !named(r#"<html lang=en><label>A</label><input name=n></html>"#),
        "a closed label names nothing after it"
    );
}

#[test]
fn the_contrast_check_reads_the_hubs_own_colour_tokens() {
    // Not a vacuous pass: a faint rule written with the Hub's own tokens is
    // caught, in both of the Hub's colourways.
    let page = format!(
        "<html lang=en><style>{}.probe{{color:var(--edge);background:var(--bg)}}</style><p class=probe>x</p></html>",
        atlas::hub::STYLE
    );
    let f = review(&page, &Rules::default());
    let probe: Vec<_> = f
        .iter()
        .filter(|f| f.rule == "contrast" && f.detail.contains(".probe"))
        .collect();
    assert!(probe.iter().any(|f| f.detail.contains("light")), "{f:?}");
    assert!(probe.iter().any(|f| f.detail.contains("dark")), "{f:?}");
}
