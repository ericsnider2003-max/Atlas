//! "What needs me?" (Plan A, 24 Oct 2026): the chief-of-staff list, on a
//! real Atlas with real state. What's soon comes first, then what waits for
//! your answer, then what stopped -- each saying why now, three at most;
//! "not now" puts them off; and asking changes nothing in Atlas but your own
//! "not now".

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn home(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-needs-you-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

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

/// A busy afternoon: the dentist in 20 minutes, a send waiting for your
/// yes, and a backup that failed.
fn busy(d: &mut Daemon, now: u64) {
    d.calendar.add(
        "Dentist",
        atlas::calendar::When {
            start: now + 20 * 60,
            end: now + 50 * 60,
            all_day: false,
        },
        None,
        now,
    );
    let mut ask = atlas::job::Record::planned(
        format!("{}invoice", atlas::job::APPROVAL_PREFIX),
        "Send the invoice to Harbor Lane",
    );
    ask.transition(
        atlas::job::State::AwaitingApproval,
        "It's a send, so it waits for your yes.",
    )
    .unwrap();
    ask.checkpoint = Some("It's a send, so it waits for your yes.".into());
    atlas::job::upsert(&d.store, ask).unwrap();
    let mut backup = atlas::job::Record::planned("backup:photos", "backup");
    backup
        .transition(atlas::job::State::Running, "started")
        .unwrap();
    atlas::job::upsert(&d.store, backup).unwrap();
    atlas::job::end(
        &d.store,
        "backup:photos",
        "backup",
        atlas::job::State::Failed,
        "The drive was full.",
    )
    .unwrap();
}

fn fresh<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(
        c,
        p,
        None,
        Store::new(home(tag)),
        Proactive::new(ProactiveConfig::default()),
    )
}

#[test]
fn what_needs_you_is_soonest_first_each_with_why_now_and_not_now_puts_it_off() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = fresh(&c, &p, "busy");
    let now = atlas::store::now();
    busy(&mut d, now);

    let said = d.turn("What needs me?", now);
    assert!(said.starts_with("Three things need you."), "{said}");
    let dentist = said.find("Dentist").expect(&said);
    let invoice = said.find("Send the invoice").expect(&said);
    let backup = said.find("backup").expect(&said);
    assert!(
        dentist < invoice && invoice < backup,
        "soon, then your move, then what stopped: {said}"
    );
    assert!(said.contains("in 20 minutes"), "{said}");
    assert!(
        said.contains("It's a send, so it waits for your yes."),
        "{said}"
    );
    assert!(said.contains("The drive was full."), "{said}");

    let off = d.turn("not now", now + 30);
    assert!(off.starts_with("Put off until"), "{off}");
    assert_eq!(
        d.turn("what needs me", now + 60),
        "Nothing needs you right now."
    );
}

#[test]
fn not_now_with_no_list_just_said_is_not_taken_as_a_put_off() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = fresh(&c, &p, "no-list");
    let now = atlas::store::now();
    busy(&mut d, now);
    let off = d.turn("not now", now);
    assert!(!off.starts_with("Put off until"), "{off}");
    // The list is still all there.
    assert!(d
        .turn("what needs me?", now + 5)
        .starts_with("Three things need you."));
}

#[test]
fn asking_changes_nothing_but_your_own_not_now() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = fresh(&c, &p, "read-only");
    let now = atlas::store::now();
    busy(&mut d, now);
    let jobs =
        |d: &Daemon| serde_json::to_string(&atlas::job::load_checked(&d.store).unwrap()).unwrap();
    let before = (jobs(&d), d.scheduler.jobs.len(), format!("{:?}", d.outbox));
    d.turn("what needs me?", now);
    d.turn("what's waiting on me?", now + 1);
    let after = (jobs(&d), d.scheduler.jobs.len(), format!("{:?}", d.outbox));
    assert_eq!(before, after, "asking what needs you changed Atlas");
}

#[test]
fn the_list_never_sends_pays_signs_or_deletes() {
    // Source guard, in the style of the repository's other guards: neither
    // half of the list calls anything that acts on the world.
    for f in ["src/needs_you.rs", "src/daemon/needs_you.rs"] {
        let src = std::fs::read_to_string(f).unwrap();
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for act in [
            ".send(",
            "send_mail",
            "remove_file",
            "remove_dir",
            "delete(",
            "pay(",
            "sign(",
            "approve(",
            "transfer",
            "turn_from(",
            "execute(",
        ] {
            assert!(!code.contains(act), "{f} calls {act}");
        }
    }
}
