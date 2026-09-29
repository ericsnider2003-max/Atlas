//! H2: the panels Atlas decides to show are drawn. Until 27 Sep 2026 "I'm
//! here", "show me my tasks" and "what are you doing" set `wants_panel` and
//! nothing ever drew it, so the morning brief's waking mark never appeared on
//! "I'm here". `panel_contents` is where that decision becomes a window; the
//! window itself can't open in a test (no screen), so this checks what it
//! would draw, and the door into the hub each one carries.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::panel::Panel as P;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use atlas::window::{Contents, Panel as W};
use std::path::Path;

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    let root = std::env::temp_dir().join(format!("atlas-panels-drawn-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("data/state")).unwrap();
    Daemon::new(c, p, None, Store::new(root.join("data/state")), Proactive::new(ProactiveConfig::default()))
}

#[test]
fn im_here_draws_the_brief_with_the_waking_mark_even_when_nothing_is_waiting() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut d = daemon(&c, &p, "waking");
    let (panel, title, lines) = d.panel_contents(P::Waking).expect("the waking moment draws nothing");
    assert_eq!(panel, W::Brief, "the brief is the panel whose mark wakes");
    assert!(title.ends_with('.'), "the greeting: {title}");
    assert_eq!(lines, vec!["Nothing needs you right now.".to_string()]);
    let (panel, _, _) = d.panel_contents(P::Tasks).unwrap();
    assert_eq!(panel, W::Outstanding);
    assert!(d.panel_contents(P::Mind).is_none(), "nothing underway, so there's no thinking to show");
    assert!(d.panel_contents(P::Controls).is_none() && d.panel_contents(P::Presence).is_none());
}

#[test]
fn every_panel_but_the_private_knock_opens_its_hub_page() {
    for (w, page) in [(W::Brief, "/hub"), (W::Outstanding, "/hub/outstanding"), (W::Thinking, "/hub/now")] {
        assert_eq!(Contents::new(w, "t", vec!["x".into()]).open.as_deref(), Some(page));
        assert!(atlas::hub::route(page).is_some() || page == "/hub", "{page} isn't a hub page");
    }
    assert_eq!(Contents::knock("Something private").open, None, "a knock carries its detail into the hub");
    // An old staged panel (before `open`) still reads.
    let old = r#"{"panel":"brief","title":"t","lines":["x"],"footer":null}"#;
    assert_eq!(serde_json::from_str::<Contents>(old).unwrap().open, None);
}
