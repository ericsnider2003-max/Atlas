//! The vault re-locks itself on the tick, not just in principle.
//!
//! `vault::should_lock` and `vault::lock` both existed and were tested in
//! isolation (`tests/vault_walk.rs`), but nothing on the running daemon ever
//! asked the question. An ordinary `atlas vault` unlock therefore stayed open
//! for the life of the process: the fifteen-minute `lock_after_mins` was a
//! setting the config carried and nothing honoured. The only production
//! `vault.lock()` was in `take_it_back`, right after an identity check.
//!
//! `Daemon::tick` now asks `should_lock` among the security housekeeping (the
//! same block as the instance-lock heartbeat, above the pause check) and locks
//! when it says so. This proves the wired behaviour: an open vault, left
//! untouched, is sealed again by the passage of ticks alone.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use atlas::vault::{State, VaultConfig};
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-vault-relock-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

#[test]
fn an_open_vault_is_sealed_again_by_the_tick_after_the_idle_limit() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = daemon(&c, &p, "idle");

    // Opened at t=0. Twelve characters at least, or `open` refuses it.
    d.vault.open("a genuinely long passphrase, not a word", 0, &VaultConfig::default()).unwrap();
    assert_eq!(d.vault.state(), State::Open, "the unlock should have opened it");

    // A minute later: still open. The whole point is that a short pause is not
    // a lockout, so a tick well inside the fifteen-minute window must leave it
    // alone.
    d.tick(60);
    assert_eq!(d.vault.state(), State::Open, "one minute idle must not re-lock it");

    // Twenty minutes after opening: past `lock_after_mins` (15). The tick, and
    // nothing the user did, seals it -- which is the capability that was dead
    // until `should_lock` was wired into the loop.
    d.tick(20 * 60);
    assert_eq!(d.vault.state(), State::Sealed, "past the idle limit the tick must re-lock it");
}
