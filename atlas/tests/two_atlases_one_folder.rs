//! `sync.rs` was 443 lines of working merge with nowhere to put a bundle.
//!
//! The append-only log, the bundle and its version check, `already_seen` so a
//! folder read twice doesn't double anything, and the conflict rule that only
//! `Changed` and `Removed` can clash — all built, all tested, and nothing in
//! Atlas ever appended an event or wrote a bundle. `Intent::Sync` answered:
//!
//! ```text
//! I'd send it ... — but moving files between your devices isn't built yet,
//! so nothing has gone anywhere.
//! ```
//!
//! Honest about itself, which is the better kind of stub, and still a
//! capability the tree had already paid for and wasn't using.
//!
//! The carrier is a folder both machines can see — the `Carry::CloudFolder`
//! this module already names. Nothing runs on the other machine and no
//! network of its own is needed, which is the right shape for a system whose
//! first rule is offline and in-house.

use atlas::backlog::Blocker;
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

const NOW: u64 = 1_700_000_000;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-sync-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

/// A config for one named machine, carrying through `folder`.
fn machine(name: &str, folder: &Path) -> Config {
    let mut c = Config::load(Path::new("config")).unwrap();
    let t = c.tools.as_mut().expect("tools section");
    t.sync.enabled = true;
    t.sync.name = name.into();
    t.sync.folder = folder.to_string_lossy().into_owned();
    c
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

// ---------- it ships ready ----------

#[test]
fn sync_ships_on_but_carries_nowhere_until_you_name_a_folder() {
    // On, because the machinery is free and local. Silent, because a folder
    // is a choice only you can make — and saying so is better than inventing
    // one under your home directory.
    let c = Config::load(Path::new("config")).unwrap();
    let s = c.tools.as_ref().unwrap().sync.clone();
    assert!(s.enabled, "sync still ships disabled");
    assert!(s.folder.trim().is_empty(), "a folder was chosen for you");

    let p = plat();
    let mut d = daemon(&c, &p, "nofolder");
    let said = d.turn("sync", NOW);
    assert!(
        said.contains("nowhere to put it") || said.contains("sync.folder"),
        "with no folder set, Atlas should say so plainly: {said}"
    );
    assert!(
        !said.contains("isn't built yet"),
        "still reporting itself as unbuilt: {said}"
    );
}

#[test]
fn a_bundle_never_carries_anything_that_opens_something_else() {
    // Not a setting, and no longer a field either: `bundles_carry_secrets`
    // was `#[serde(skip)]` and pinned false, which made it a constant wearing
    // a config field, and nothing read it. Deleted 19 Sep 2026 -- what keeps
    // the promise is that `sync::What` has five variants and none of them can
    // hold a credential, which `tests/sync.rs` pins.
    //
    // What still has to hold here is the other half: the shipped file must
    // not invite anyone to look for a switch that never existed.
    let c = Config::load(Path::new("config")).unwrap();
    assert!(c.tools.is_some(), "tools.yaml still loads");
    let raw = std::fs::read_to_string("config/tools.yaml").unwrap();
    assert!(
        !raw.contains("bundles_carry_secrets"),
        "the secrets flag appears in the shipped config"
    );
}

// ---------- it actually carries ----------

#[test]
fn what_you_capture_on_one_machine_reaches_the_other() {
    // The whole point, end to end, through nothing but a shared folder.
    let folder = tmp("shared");
    let p = plat();

    let laptop_cfg = machine("laptop", &folder);
    let mut laptop = daemon(&laptop_cfg, &p, "laptop");
    laptop.turn("note that the rack needs a 10-inch shelf", NOW);
    let left = laptop.turn("sync", NOW + 10);
    assert!(
        left.contains("Left") && left.contains(&folder.display().to_string()),
        "the laptop left nothing in the folder: {left}"
    );

    // A second machine, its own state, pointed at the same folder.
    let desk_cfg = machine("desktop", &folder);
    let mut desk = daemon(&desk_cfg, &p, "desktop");
    let took = desk.turn("sync", NOW + 20);

    assert!(
        took.contains("Took in"),
        "the desktop read the folder and took nothing: {took}"
    );
}

#[test]
fn reading_the_same_folder_twice_does_not_double_anything() {
    // `already_seen` exists for exactly this, and a folder is read every time
    // you say "sync" — so this is the normal case, not an edge one.
    let folder = tmp("twice");
    let p = plat();

    let laptop_cfg = machine("laptop", &folder);
    let mut laptop = daemon(&laptop_cfg, &p, "laptop2");
    laptop.turn("note that the rack needs a 10-inch shelf", NOW);
    laptop.turn("sync", NOW + 10);

    let desk_cfg = machine("desktop", &folder);
    let mut desk = daemon(&desk_cfg, &p, "desktop2");
    let first = desk.turn("sync", NOW + 20);
    let second = desk.turn("sync", NOW + 30);

    assert!(first.contains("Took in"), "nothing arrived the first time: {first}");
    assert!(
        !second.contains("Took in"),
        "the same bundle was taken in twice: {second}"
    );
}

#[test]
fn a_machine_does_not_take_in_its_own_bundle() {
    // Both machines write into the same folder, so every read sees our own
    // bundle sitting there. Skipping it is what makes a shared folder work.
    let folder = tmp("own");
    let p = plat();
    let c = machine("laptop", &folder);
    let mut d = daemon(&c, &p, "own");

    d.turn("note that the rack needs a 10-inch shelf", NOW);
    d.turn("sync", NOW + 10);
    let again = d.turn("sync", NOW + 20);

    assert!(
        !again.contains("Took in"),
        "a machine read its own bundle back in: {again}"
    );
}

#[test]
fn switching_it_off_stops_it() {
    let folder = tmp("off");
    let p = plat();
    let mut c = machine("laptop", &folder);
    c.tools.as_mut().unwrap().sync.enabled = false;
    let mut d = daemon(&c, &p, "syncoff");

    let said = d.turn("sync", NOW);
    assert!(said.contains("switched off"), "sync ran with the setting off: {said}");
    assert!(
        std::fs::read_dir(&folder).map(|d| d.count()).unwrap_or(0) == 0,
        "something was written to the folder with sync off"
    );
}

// ---------- the log is real ----------

#[test]
fn the_backlog_is_not_what_gets_carried() {
    // Guards the design decision rather than the code: sync carries *what
    // happened*, not state. If someone later starts shipping the backlog
    // itself across, the copies can disagree again and this is the test that
    // should stop them.
    let folder = tmp("shape");
    let p = plat();
    let c = machine("laptop", &folder);
    let mut d = daemon(&c, &p, "shape");

    d.backlog.record("retry the backup", Blocker::NeedsApproval, NOW);
    d.turn("sync", NOW + 10);

    let bundle = std::fs::read_dir(&folder)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().and_then(|x| x.to_str()) == Some("bundle"))
        .expect("a bundle was written");
    let raw = std::fs::read_to_string(bundle).unwrap();

    assert!(
        !raw.contains("retry the backup"),
        "the backlog was carried across as state:\n{raw}"
    );
}

// ---------- your two Atlases are not your two machines ----------

/// The same, but declaring which Atlas it is.
fn atlas_named(name: &str, belongs_to: &str, folder: &Path) -> Config {
    let mut c = machine(name, folder);
    c.tools.as_mut().unwrap().sync.belongs_to = belongs_to.into();
    c
}

#[test]
fn a_work_atlas_and_the_personal_one_do_not_carry_each_other() {
    // Decided 18 Sep 2026: a second Atlas with a different job does not sync
    // with the personal one — it gets linked through the business hub,
    // deliberately and item by item, which is a different thing from
    // replaying one log into another.
    //
    // Recorded here rather than only in a handover, because until this the
    // only thing keeping them apart was nobody pointing both at the same
    // folder, and "nobody has made that mistake yet" is not a boundary.
    let folder = tmp("estates");
    let p = plat();

    let work_cfg = atlas_named("vps", "homelab", &folder);
    let mut work = daemon(&work_cfg, &p, "work");
    work.turn("note that the client invoice went out at 9:40", NOW);
    work.turn("sync", NOW + 10);

    let personal_cfg = atlas_named("laptop", "personal", &folder);
    let mut personal = daemon(&personal_cfg, &p, "personal");
    let said = personal.turn("sync", NOW + 20);

    assert!(
        !said.contains("Took in"),
        "the work Atlas's notes were merged into the personal one: {said}"
    );
    assert!(
        said.contains("homelab") && said.contains("left it alone"),
        "it should say plainly whose bundle it skipped: {said}"
    );
}

#[test]
fn two_machines_of_the_same_atlas_still_carry_each_other() {
    // The control. The boundary must separate your two *Atlases*, not your
    // two machines — or it would have broken the thing it was added to.
    let folder = tmp("same-estate");
    let p = plat();

    let laptop_cfg = atlas_named("laptop", "personal", &folder);
    let mut laptop = daemon(&laptop_cfg, &p, "e-laptop");
    laptop.turn("note that the rack needs a 10-inch shelf", NOW);
    laptop.turn("sync", NOW + 10);

    let desk_cfg = atlas_named("desktop", "personal", &folder);
    let mut desk = daemon(&desk_cfg, &p, "e-desktop");
    let said = desk.turn("sync", NOW + 20);

    assert!(said.contains("Took in"), "two of your own machines stopped carrying: {said}");
}

#[test]
fn a_bundle_written_before_this_existed_still_opens_as_personal() {
    // `belongs_to` is defaulted rather than required, because a bundle from
    // six months ago has to still open — and every bundle written before this
    // field existed was a personal one.
    let raw = r#"{"from_device":"old","from_name":"old","made_at":1,
                  "up_to_seq":0,"events":[],"version":1}"#;
    let b: atlas::sync::Bundle = serde_json::from_str(raw).expect("an old bundle still parses");
    assert_eq!(b.belongs_to, "personal");
    assert!(atlas::sync::from_the_same_atlas(&b, "personal").is_ok());
    assert!(atlas::sync::from_the_same_atlas(&b, "homelab").is_err());
}
