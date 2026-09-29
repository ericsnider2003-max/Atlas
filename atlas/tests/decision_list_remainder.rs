//! The rest of the 23 Sep decision list: "argue the other side" as a real
//! intent, and the two trading primitives that needed a reader rather than a
//! wire (`AsOf::back_to`, `Event::is_window`).

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::market::bars::Bars;
use atlas::market::events::{Event, Tier};
use atlas::market::fixtures::path;
use atlas::market::structure::{before_the_turn, recent};
use atlas::otherside::{argued, decision_from, not_raised, against, Angle};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::standdown::Blackout;
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-remainder-{tag}"));
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

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

// ================= argue the other side =================

#[test]
fn the_decision_is_heard_in_what_was_said() {
    let d = decision_from(
        "argue the other side: I'm selling the car for good because everyone says EVs are the future",
        None,
    )
    .expect("a strong trigger with a decision");
    assert_eq!(d.what, "I'm selling the car for good");
    assert!(d.because.contains("everyone says"));
    assert!(!d.reversible, "'for good' is the speaker saying it can't be undone");

    let d = decision_from(
        "talk me out of the new laptop, it costs £1400 and it only works as long as the contract renews",
        None,
    )
    .unwrap();
    assert_eq!(d.costs.as_deref(), Some("£1400"));
    assert_eq!(d.depends_on, vec!["the contract renews".to_string()]);
    assert!(d.reversible, "nothing said makes it permanent, so nothing is assumed");
    assert!(d.previously.is_none(), "history is never invented");
}

#[test]
fn a_fault_report_is_not_a_request_to_argue() {
    // "what's wrong with" is a weak trigger: only a decision after it counts.
    assert!(decision_from("what's wrong with the printer", None).is_none());
    assert!(decision_from("what's wrong with switching to a standing desk", None).is_some());
    // And never volunteered.
    assert!(decision_from("I'm switching to a standing desk", None).is_none());
}

#[test]
fn on_its_own_it_argues_the_last_thing_said() {
    let d = decision_from("argue the other side", Some("I'm going to quit the gym")).unwrap();
    assert_eq!(d.what, "I'm going to quit the gym");
    assert!(decision_from("argue the other side", None).is_none(), "nothing to argue against");
}

#[test]
fn angles_without_evidence_are_named_not_hidden() {
    let d = decision_from("make the case against moving to Leeds because the rent is lower", None)
        .unwrap();
    let objections = against(&d);
    let missing = not_raised(&objections);
    assert!(missing.contains(&Angle::YouTriedThis), "no history, so not argued");
    assert!(missing.iter().all(|a| a.needs_evidence()));
    assert!(Angle::YouTriedThis.needs_evidence());
    assert!(!Angle::NothingIsFine.needs_evidence(), "doing nothing needs no evidence to ask");
    let said = argued(&d);
    assert!(said.starts_with("The strongest case against"), "{said}");
    assert!(said.contains("I didn't argue"), "{said}");
    assert!(said.contains("tried this before"), "{said}");

    // With a stated cost the cost angle IS raised, so it drops out of the gaps.
    let costed =
        decision_from("make the case against the course, it costs $900", None).unwrap();
    let raised = against(&costed);
    assert!(raised.iter().any(|o| o.angle == Angle::NotWorthIt));
    assert!(!not_raised(&raised).contains(&Angle::NotWorthIt));
    assert_ne!(argued(&costed), argued(&d), "the answer reads the decision");
}

#[test]
fn asking_through_the_daemon_gets_the_case_against() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "otherside");
    let reply = d.turn(
        "argue the other side: I'm quitting my job for good because it just makes sense",
        100,
    );
    assert!(reply.contains("strongest case against"), "{reply}");
    assert!(reply.contains("can't be undone"), "irreversible is the heaviest angle: {reply}");
}

// ================= before the turn (AsOf::back_to) =================

const UP_THEN_REVERSAL: [f64; 10] = [
    1.0000, 1.0100, 1.0050, 1.0200, 1.0150, 1.0300, 1.0250, 1.0280, 1.0200, 1.0230,
];
const UP_THEN_PULLBACK: [f64; 10] = [
    1.0000, 1.0100, 1.0050, 1.0200, 1.0150, 1.0300, 1.0250, 1.0280, 1.0260, 1.0290,
];

fn bars(points: &[f64]) -> Bars {
    Bars::from_closes(&path(points, 0.0002), 0.0006).unwrap()
}

#[test]
fn a_break_is_shown_against_what_could_be_read_before_it() {
    let b = bars(&UP_THEN_REVERSAL);
    let v = b.latest().unwrap();
    let now = recent(&v, 5, 2);
    let at = now.reversal().unwrap().at.unwrap();
    let before = before_the_turn(&v, 5, 2).expect("it turned, so there is a before");
    assert!(before.contains(&format!("bar {at}")), "{before}");
    // The earlier reading is the uptrend the break turned, read through a
    // view that stops before the break -- via back_to, bounded, not redrawn.
    let earlier = v.back_to(at - 1).unwrap();
    assert!(earlier.len() < v.len());
    assert!(before.contains(&recent(&earlier, 5, 2).say()), "{before}");
    // Before the break the low still held, so it read as a warning -- the
    // reading a person had in front of them when the break came.
    assert!(before.contains("MAY be turning"), "{before}");
    assert!(!before.contains("turned DOWN"), "the before cannot contain the break: {before}");
    // And a bounded view can't be widened.
    assert!(earlier.back_to(v.len()).is_err());
}

#[test]
fn nothing_turned_means_no_before() {
    let b = bars(&UP_THEN_PULLBACK);
    let v = b.latest().unwrap();
    assert!(before_the_turn(&v, 5, 2).is_none(), "an unconfirmed warning has no break bar");
}

// ================= a release with no fixed time (Event::is_window) =================

fn event(name: &str, window_end: Option<i64>) -> Event {
    Event {
        name: name.into(),
        at: 1_780_000_000_000,
        currency: "JPY",
        impact: 3,
        tier: Tier::Fixed,
        window_end,
    }
}

#[test]
fn a_banded_release_is_read_as_the_band() {
    let boj = event("BoJ Policy Rate", Some(1_780_003_600_000));
    let fixed = event("Tokyo CPI", None);
    assert!(boj.is_window());
    assert!(!fixed.is_window());
    let banded = Blackout::Now(vec![boj.clone()]).plain();
    assert!(banded.contains("has no fixed time"), "{banded}");
    assert!(banded.contains("BoJ Policy Rate has no fixed time"), "{banded}");
    let point = Blackout::Now(vec![fixed.clone()]).plain();
    assert!(!point.contains("no fixed time"), "a timed release is a point: {point}");
    // Both readings, bar and now, carry it.
    assert!(Blackout::InsideTheBar(vec![fixed, boj]).plain().contains("no fixed time"));
}
