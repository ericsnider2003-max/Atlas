//! "Do that tonight." "When I'm out."
//!
//! Atlas could be told what it could not do — the backlog holds a thing that
//! was offline, or needed the screen, or needed your yes. It had no way to be
//! told what it *should not do yet*. Every request was either done now or not
//! at all, so "back up the drive tonight" was a sentence Atlas heard and then
//! acted on immediately, which is the opposite of what it says.
//!
//! The night already worked the backlog. What was missing was a way in.

use atlas::backlog::{Blocker, Conditions};
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

const NOW: u64 = 1_700_000_000;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-hold-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
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

// ---------- saying it ----------

#[test]
fn asking_for_it_tonight_parks_it_instead_of_doing_it() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "tonight");

    let said = d.turn("rebuild the index tonight", NOW);

    assert!(said.contains("hold"), "it did not say it was holding: {said}");
    assert_eq!(d.backlog.outstanding().len(), 1, "nothing was parked");
    assert_eq!(d.backlog.outstanding()[0].blocker, Blocker::NotWhileYouAreHere);
}

#[test]
fn the_same_request_without_the_words_runs_now() {
    // The control. If this also parked, the feature would be a bug.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "now");

    let said = d.turn("rebuild the index", NOW);

    assert!(!said.contains("hold"), "an ordinary request was parked: {said}");
    assert!(d.backlog.outstanding().is_empty(), "an ordinary request was parked");
}

#[test]
fn a_question_containing_the_word_is_still_answered() {
    // "What did I get done tonight" is a question, not an instruction to
    // wait. Parking it would make Atlas mute on the word.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "question");

    let said = d.turn("what did I get done tonight", NOW);

    assert!(!said.contains("hold"), "a question was parked as work: {said}");
    assert!(d.backlog.outstanding().is_empty(), "a question was parked as work");
}

// ---------- it waits for the right thing ----------

#[test]
fn it_stays_blocked_while_you_are_here_and_clears_when_you_go() {
    let b = atlas::backlog::Backlog::default();
    let item = atlas::backlog::Item {
        id: 1,
        request: "rebuild the index".into(),
        blocker: Blocker::NotWhileYouAreHere,
        first_seen: NOW,
        last_offered: 0,
        offers: 0,
        dismissed: false,
        done: false,
    };
    let here = Conditions { online: true, screen_free: true, you_are_here: true, tools: vec![] };
    let away = Conditions { you_are_here: false, ..here.clone() };

    assert!(b.is_blocked(&item, &here), "it would have run while you were sitting there");
    assert!(!b.is_blocked(&item, &away), "it never clears, so it would never run");
}

#[test]
fn it_clears_on_its_own_so_nothing_has_to_ask_you_again() {
    // The difference between this and `NeedsApproval`: you already said yes,
    // you just said "not now". Asking again when you step away would be
    // asking twice for one decision.
    assert!(Blocker::NotWhileYouAreHere.self_clearing());
}

// ---------- a bundle is yours ----------

