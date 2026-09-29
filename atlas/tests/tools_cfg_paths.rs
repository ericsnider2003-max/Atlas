//! `ToolsConfig.work_dir` ships as a bare relative default ("data/tmp") --
//! the same shape of bug `backup.dir` and `research.notes_dir` had before
//! `Store::install_root`. Proven here at the `Daemon::tools_cfg()` level,
//! since that's the one place all five real call sites read it from.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-tools-cfg-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, root: PathBuf) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(root), Proactive::new(ProactiveConfig::default()))
}

#[test]
fn two_isolated_installs_never_share_a_scratch_folder() {
    let c = cfg();
    let p = plat();
    let a = daemon(&c, &p, tmp("a"));
    let b = daemon(&c, &p, tmp("b"));

    assert_ne!(
        a.tools_cfg().work_dir,
        b.tools_cfg().work_dir,
        "two isolated stores must never resolve to the same scratch directory"
    );
}

#[test]
fn the_scratch_dir_lives_under_this_installs_own_root() {
    let c = cfg();
    let p = plat();
    let root = tmp("under-root");
    let d = daemon(&c, &p, root.clone());

    assert!(
        d.tools_cfg().work_dir.starts_with(root.to_string_lossy().as_ref()),
        "the scratch dir must live under this install's own root, got: {}",
        d.tools_cfg().work_dir
    );
}
