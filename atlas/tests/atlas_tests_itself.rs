//! **Atlas tests itself (30 Sep 2026).** `selftest::run_all` asks for every
//! command on a stand-in platform with no model: nothing is broken, the
//! screen is only recorded, background work is only named, and the
//! commands that would touch real files are rehearsed.

use atlas::config::Config;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::selftest::{run_all, SafePlatform, Tier, Verdict};
use std::path::Path;

#[test]
fn every_command_is_tried_and_nothing_is_broken() {
    atlas::handover::Handover::default().save(&atlas::roots::install_state()).unwrap();
    let c = Config::load(Path::new("config")).unwrap();
    let real = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let plat = SafePlatform::wrapping(&real);
    let dir = std::env::temp_dir().join(format!("atlas-selftest-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let rows = run_all(&c, &plat, None, &dir, 1_790_760_000, &mut |_| {});
    atlas::handover::Handover::default().save(&atlas::roots::install_state()).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(rows.len() > 150, "{}", rows.len());
    let broken: Vec<String> = rows.iter().filter(|r| r.verdict.is_a_fault()).map(|r| format!("{} {:?} -> {} ({})", r.command, r.said, r.verdict.plain(), r.reply)).collect();
    assert!(broken.is_empty(), "{}", broken.join("\n"));
    // Opening an app was done to the stand-in, written down, not done.
    let open = rows.iter().find(|r| r.command == "everyday: open_app").unwrap();
    assert!(open.would.iter().any(|w| w.starts_with("start ")), "{:?}", open.would);
    // Whatever writes your files was rehearsed.
    let tidy = rows.iter().find(|r| r.command == "tidy_desktop").unwrap();
    assert_eq!(tidy.tier, Tier::Rehearse);
    assert_eq!(tidy.verdict, Verdict::Rehearsed);
    let md = atlas::selftest::report(&rows, "today", false);
    assert!(md.contains("# Atlas tested itself") && md.contains("## Works"), "{md}");
}
