//! Teaching Atlas a mode in your own words (Plan B, 24 Oct 2026): taught,
//! read back, kept only on your yes, then run by name like any mode -- and
//! never holding a step that sends, pays or deletes.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::Path;

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

fn fresh<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    let root = std::env::temp_dir().join(format!("atlas-teach-mode-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    Daemon::new(
        c,
        p,
        None,
        Store::new(&root),
        Proactive::new(ProactiveConfig::default()),
    )
}

#[test]
fn a_mode_taught_in_plain_words_is_read_back_kept_on_yes_and_runs_by_name() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = fresh(&c, &p, "taught");
    let t = atlas::store::now();
    let back = d.turn("When I say writing mode, close Discord and open Notepad", t);
    assert_eq!(
        back,
        "When you say \"writing mode\", I'll close discord, then open notepad. Keep it? Say yes or no."
    );
    assert!(d.modes.get("writing").is_none(), "kept before the yes");
    let kept = d.turn("yes", t + 5);
    assert_eq!(kept, "Done. Say \"writing mode\" whenever you want it.");
    let m = d.modes.get("writing").expect("kept on yes");
    assert_eq!(
        (m.open.clone(), m.close.clone()),
        (vec!["notepad".to_string()], vec!["discord".to_string()])
    );
    // Kept across a restart.
    assert!(atlas::modes::Modes::load(&d.store).get("writing").is_some());
    // And it runs by name, through the mode machinery every mode uses.
    d.turn("writing mode", t + 10);
    assert_eq!(d.modes.active().map(|m| m.name.as_str()), Some("writing"));
}

#[test]
fn no_keeps_nothing_and_a_later_yes_is_not_taken() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = fresh(&c, &p, "no");
    let t = atlas::store::now();
    d.turn(
        "make a mode called reading that opens kindle and goes quiet",
        t,
    );
    assert_eq!(d.turn("no", t + 5), "Left it -- no reading mode.");
    assert!(d.modes.get("reading").is_none());
    // A yes much later, to something else, is not a yes to this.
    d.turn("make a mode called gaming that opens steam", t + 10);
    let later = d.turn("yes", t + 10 + 6 * 60);
    assert!(!later.starts_with("Done."), "{later}");
    assert!(d.modes.get("gaming").is_none());
}

#[test]
fn a_step_that_sends_pays_or_deletes_is_refused_and_nothing_is_kept() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = fresh(&c, &p, "refused");
    let t = atlas::store::now();
    let said = d.turn(
        "when I say morning mode, open mail and send Sam the report",
        t,
    );
    assert!(
        said.starts_with("A mode can open apps, close apps and keep me quiet"),
        "{said}"
    );
    assert!(said.contains("not \"send sam the report\""), "{said}");
    let yes = d.turn("yes", t + 5);
    assert!(
        !yes.starts_with("Done."),
        "a refused mode was kept on yes: {yes}"
    );
    assert!(d.modes.get("morning").is_none());
}

#[test]
fn moving_on_drops_the_read_back_so_a_later_yes_is_not_a_yes_to_it() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = fresh(&c, &p, "moved-on");
    let t = atlas::store::now();
    d.turn("when I say gaming mode, open steam", t);
    d.turn("what time is it", t + 5);
    let yes = d.turn("yes", t + 10);
    assert!(!yes.starts_with("Done."), "{yes}");
    assert!(d.modes.get("gaming").is_none());
}
