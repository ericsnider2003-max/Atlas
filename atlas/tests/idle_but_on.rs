//! A switch that is on but has nothing to run on should be named on the
//! settings page, so the person can turn it off instead of paying for it.

use atlas::hub::{idle_banner, settings_page, IDLE_TOGGLES};
use atlas::settings::{registry, Settings};
use atlas::voice::ToolsConfig;

fn fresh() -> Settings {
    registry(&ToolsConfig::default())
}

fn blocked(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

#[test]
fn idle_but_on_names_a_toggle_that_is_on_and_listed() {
    let mut s = fresh();
    s.set("ocr.enabled", "on").unwrap();
    let keys = vec!["ocr.enabled".to_string()];
    let idle = s.idle_but_on(&keys);
    assert_eq!(idle.len(), 1);
    assert_eq!(idle[0].key, "ocr.enabled");

    s.set("ocr.enabled", "off").unwrap();
    assert!(s.idle_but_on(&keys).is_empty(), "an off switch is not idle");
}

#[test]
fn a_blocked_capability_with_its_switch_on_is_named() {
    let mut s = fresh();
    s.set("ocr.enabled", "on").unwrap();
    let name = s.get("ocr.enabled").unwrap().name.clone();
    let banner = idle_banner(&s, &blocked(&["ocr"])).expect("should say something");
    assert!(banner.contains(&name), "got: {banner}");
}

#[test]
fn the_same_switch_off_is_not_named() {
    let mut s = fresh();
    s.set("ocr.enabled", "off").unwrap();
    assert_eq!(idle_banner(&s, &blocked(&["ocr"])), None);
}

#[test]
fn nothing_blocked_means_no_banner() {
    let mut s = fresh();
    s.set("ocr.enabled", "on").unwrap();
    s.set("dictate.enabled", "on").unwrap();
    assert_eq!(idle_banner(&s, &[]), None);
    // Blocked, but nothing mapped to it is a switch.
    assert_eq!(idle_banner(&s, &blocked(&["reason"])), None);
}

#[test]
fn only_the_blocked_ones_are_named() {
    let mut s = fresh();
    s.set("ocr.enabled", "on").unwrap();
    s.set("dictate.enabled", "on").unwrap();
    let ocr = s.get("ocr.enabled").unwrap().name.clone();
    let dictate = s.get("dictate.enabled").unwrap().name.clone();
    let banner = idle_banner(&s, &blocked(&["dictate"])).unwrap();
    assert!(banner.contains(&dictate), "got: {banner}");
    assert!(!banner.contains(&ocr), "ocr isn't blocked here: {banner}");
}

#[test]
fn every_mapped_key_is_a_real_toggle() {
    let s = fresh();
    for (cap, key) in IDLE_TOGGLES {
        let item = s.get(key).unwrap_or_else(|| panic!("{key} (for {cap}) isn't a setting"));
        assert!(
            matches!(item.value, atlas::settings::Value::Toggle(_)),
            "{key} isn't a switch"
        );
    }
}

#[test]
fn the_settings_page_shows_the_banner_for_what_is_really_blocked() {
    // `ocr` ships Blocked in the capability table; this checks the page asks
    // that table rather than being handed a list nobody passes.
    let ocr_blocked = atlas::capability::blocked().iter().any(|(c, _)| c.id == "ocr");
    let mut s = fresh();
    s.set("ocr.enabled", "on").unwrap();
    let page = settings_page(&s);
    assert_eq!(page.contains("On but doing nothing right now"), ocr_blocked);
}
