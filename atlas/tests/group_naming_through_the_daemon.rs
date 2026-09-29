//! Naming a group and asking who's in it, driven through a whole daemon.
//!
//! The group protocol itself is unit-tested in `messaging_between_people`. This
//! drives the two *reading* intents end to end — the ones a person actually
//! says — by seeding a group room into the store the daemon loads, then asking.
//! Reachability in the answer depends on this install's pairings (which live
//! outside the test store), so these assert on the parts that don't: the
//! members named, and the rename taking effect.

use atlas::chat::{Chats, Room};
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::earned::Space;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-grp-{tag}"));
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

fn daemon_with_group(dir: &Path) -> Config {
    // Seed a group room into the store the daemon will load.
    let store = Store::new(dir.to_path_buf());
    let mut chats = Chats::default();
    chats.rooms.push(Room {
        id: "grp-seed".into(),
        name: "Northwind".into(),
        space: Space::Personal,
        members: vec!["Jordan".into(), "Maya".into()],
        messages: vec![],
        read_through: 0,
        receipt_through: 0,
    });
    chats.save(&store).unwrap();
    cfg()
}

#[test]
fn who_is_in_a_group_names_its_members() {
    let dir = tmp("whos-in");
    let c = daemon_with_group(&dir);
    let p = plat();
    let mut d = Daemon::new(&c, &p, None, Store::new(dir), Proactive::new(ProactiveConfig::default()));
    let reply = d.turn("who's in the Northwind group", 100);
    assert!(
        reply.contains("Jordan") && reply.contains("Maya"),
        "who's-in should name the members, got: {reply}"
    );
}

#[test]
fn naming_a_group_lets_you_reach_it_by_that_name() {
    let dir = tmp("name-grp");
    let c = daemon_with_group(&dir);
    let p = plat();
    let mut d = Daemon::new(&c, &p, None, Store::new(dir), Proactive::new(ProactiveConfig::default()));
    let reply = d.turn("name the Northwind group as Roof crew", 100);
    assert!(
        reply.to_lowercase().contains("roof crew"),
        "should confirm the new name, got: {reply}"
    );
    assert!(
        d.chats.group_named("Roof crew").is_some(),
        "the group is reachable by its new name afterwards"
    );
}

#[test]
fn leaving_a_group_drops_it_and_tombstones_it() {
    let dir = tmp("leave-grp");
    let c = daemon_with_group(&dir);
    let p = plat();
    let mut d = Daemon::new(&c, &p, None, Store::new(dir), Proactive::new(ProactiveConfig::default()));
    let reply = d.turn("leave the Northwind group", 100);
    assert!(reply.to_lowercase().contains("left"), "should confirm leaving, got: {reply}");
    // The room is gone from this end...
    assert!(d.chats.group_named("Northwind").is_none(), "the group is no longer held");
    // ...and cannot be re-opened by a message still in flight.
    assert!(d.chats.has_left("grp-seed"), "the group id is tombstoned");
}

#[test]
fn asking_about_a_group_that_isnt_there_says_so() {
    let dir = tmp("no-group");
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(dir), Proactive::new(ProactiveConfig::default()));
    let reply = d.turn("who's in the Nowhere group", 100);
    assert!(reply.to_lowercase().contains("don't have a group"), "got: {reply}");
    // The missing-group answer is its own branch, not the catch-all.
    let junk = d.turn("zzqx frobnicate wibble", 100);
    assert_ne!(reply, junk, "the no-such-group answer is a branch of its own");
}
