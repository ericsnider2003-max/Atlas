//! Guests at the daemon: things a guest must not be able to get Atlas to do.
//!
//! Moved out of `asking_your_other_atlas.rs` and `holding_it_until_youre_out.rs`
//! on 26 Sep 2026. Both said "you're talking to someone else now", which was
//! not a handover phrase: the line wasn't understood, became "I didn't catch
//! that. Go ahead?", and swallowed the guest's next request, so the tests
//! passed without Atlas ever being handed over. Now the phrase hands it over
//! for real, and a handover is install-wide state, so these run in their own
//! process with their own install, one at a time.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::elsewhere::{Elsewhere, ElsewhereConfig};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, Once};

const NOW: u64 = 1_700_000_000;

/// One install, one test at a time, handed back before each.
fn alone() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    static ONCE: Once = Once::new();
    let g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    ONCE.call_once(|| {
        let p = std::env::temp_dir().join(format!("atlas-guests-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(p.join("data").join("state")).unwrap();
        std::env::set_var("ATLAS_HOME", &p);
    });
    atlas::handover::Handover::default().save(&atlas::roots::install_state()).unwrap();
    g
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-guests-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

fn knows_homelab() -> Config {
    let mut c = cfg();
    c.tools.as_mut().unwrap().elsewhere = ElsewhereConfig {
        enabled: true,
        timeout_secs: 1,
        known: vec![Elsewhere {
            name: "homelab".into(),
            host: "192.0.2.1".into(),
            port: 8787,
            token: "a-token-long-enough-to-be-real".into(),
            sync_port: None,
        }],
    };
    c
}

#[test]
fn the_guest_phrase_really_hands_it_over() {
    let _g = alone();
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "really");
    let _ = d.turn("you're talking to someone else now", NOW);
    assert!(atlas::handover::Handover::load(&atlas::roots::install_state()).stance.handed_over());
}

#[test]
fn a_guest_cannot_ask_how_your_server_is_getting_on() {
    let _g = alone();
    // The reply is a list of what your server could not do — your
    // infrastructure, read out to whoever is holding the laptop. The same
    // reasoning that makes "outstanding" and "queued" the owner's own.
    let (c, p) = (knows_homelab(), plat());
    let mut d = daemon(&c, &p, "guest");

    d.turn("you're talking to someone else now", NOW);
    let said = d.turn("how is homelab", NOW + 5);

    assert!(
        !said.contains("couldn't reach"),
        "a guest got Atlas to go and look at your server: {said}"
    );
}

#[test]
fn sync_will_not_carry_your_notes_while_somebody_else_is_at_the_machine() {
    let _g = alone();
    // A bundle is your captures. Handed over means you have said out loud
    // that somebody else is using Atlas; they may talk to it, they may not
    // push your notes into a folder. This was missing from the first version
    // of the sync wiring.
    let folder = tmp("guest-folder");
    let mut c = cfg();
    {
        let t = c.tools.as_mut().unwrap();
        t.sync.enabled = true;
        t.sync.name = "laptop".into();
        t.sync.folder = folder.to_string_lossy().into_owned();
    }
    let p = plat();
    let mut d = daemon(&c, &p, "guest");

    d.turn("note that the rack needs a 10-inch shelf", NOW);
    d.turn("you're talking to someone else now", NOW + 5);
    let said = d.turn("sync", NOW + 10);

    let wrote_anything = std::fs::read_dir(&folder)
        .map(|it| it.flatten().any(|e| e.path().extension().is_some_and(|x| x == "bundle")))
        .unwrap_or(false);
    assert!(
        !wrote_anything,
        "a guest pushed your notes into the folder. Reply was: {said}"
    );
}
