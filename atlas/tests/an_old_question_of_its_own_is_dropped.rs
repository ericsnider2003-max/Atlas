//! **A question Atlas kept in one of its own slots expires too (30 Sep
//! 2026).** Only questions asked through the session were dropped after
//! ten minutes; one left in a slot of Atlas's own could sit for days and
//! take an unrelated sentence as its answer.

use atlas::config::Config;
use atlas::daemon::{Daemon, QUESTION_LIFETIME_SECS};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::Path;

#[test]
fn a_slot_question_left_for_ten_minutes_is_dropped() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let dir = std::env::temp_dir().join(format!("atlas-old-slot-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut d = Daemon::new(&c, &p, None, Store::new(dir.clone()), Proactive::new(ProactiveConfig::default()));
    let t = 1_790_760_000u64;
    d.pending_panel = Some(atlas::panel::Panel::Tasks);
    d.tick(t);
    d.tick(t + 60);
    assert!(d.pending_panel.is_some(), "a minute old is still a live question");
    d.tick(t + QUESTION_LIFETIME_SECS + 5);
    assert!(d.pending_panel.is_none(), "ten minutes on, nobody is answering it");
    let _ = std::fs::remove_dir_all(&dir);
}
