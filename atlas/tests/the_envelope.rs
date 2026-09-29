//! The envelope arrangement, now that something holds it.
//!
//! `afterme.rs` was complete, tested and unreachable. `gaps` wants a place, a
//! list of people who have been told, and something counting the days, and
//! nothing in the tree held any of those — so nothing could call it, and
//! `after_me:` sat in `tools.yaml` being parsed into a field nobody read.
//! `config::PARSED_AND_NEVER_READ` recorded exactly that.
//!
//! What was missing was never the logic. It was somewhere to keep what you
//! have actually arranged, and a way to say it.
//!
//! The thing worth guarding here is not the feature, it is what the feature
//! must never do: Atlas does not hold the passphrase, and nobody is handed
//! anything before the condition they agreed to.

use atlas::afterme::{
    self, AfterMeConfig, Arrangement, Instruction, Timer, When, Where,
};

const DAY: u64 = 86_400;

fn told(person: &str, agreed: bool) -> Instruction {
    Instruction {
        person: person.into(),
        location: "the safe in the study".into(),
        when: When::OutOfContact { days: 180 },
        then_what: "Open it and follow what's inside.".into(),
        they_know: agreed,
    }
}

fn arranged() -> Arrangement {
    Arrangement {
        kind: Some(Where::YoursAlone),
        instructions: vec![told("Sam", true)],
        timer: Some(Timer::ThePlatforms),
        reviewed_at: 0,
    }
}

// ================= what Atlas keeps, and what it never keeps =================

#[test]
fn what_is_written_down_contains_no_secret_and_could_not() {
    // The arrangement is stored on disk. Anyone reading that file learns that
    // an envelope exists and not one thing about what is in it — and not
    // because the code is careful, because the types have nowhere to put it.
    let whole = serde_json::to_string(&arranged()).expect("it serialises");
    for secret in ["passphrase", "password", "recovery", "seed", "secret"] {
        assert!(
            !whole.contains(secret),
            "the arrangement has somewhere to put a {secret}: {whole}"
        );
    }
    // `AfterMeConfig.atlas_holds_it` used to sit here: a `#[serde(skip)]`
    // bool pinned false that nothing read. A bool nobody reads is not a
    // boundary, and it could not have become one — nothing would have
    // consulted it before deciding. What holds the guarantee is that there is
    // nowhere to put a passphrase, so a config file naming one changes
    // nothing at all.
    let cfg: AfterMeConfig = serde_yaml::from_str(
        "enabled: true\natlas_holds_it: true\npassphrase: hunter2\n",
    )
    .expect("an unknown key must not stop the section parsing");
    assert!(cfg.enabled, "the rest of the section still parsed");
    let written = serde_json::to_string(&arranged()).expect("it serialises");
    assert!(!written.contains("hunter2"), "{written}");
}

#[test]
fn a_person_is_told_where_and_when_and_nothing_else() {
    let said = told("Sam", true).as_told();
    assert!(said.contains("if I'm out of contact for 180 days"));
    assert!(said.contains("the safe in the study"));
    assert!(said.contains("Open it"));
    // Nothing in that sentence is worth anything on its own.
    assert!(!said.to_lowercase().contains("passphrase"));
}

// ================= the arrangement =================

#[test]
fn nothing_arranged_says_so_rather_than_saying_nothing() {
    let cfg = AfterMeConfig { enabled: true, ..Default::default() };
    let empty = Arrangement::default();
    let said = empty.spoken(&cfg, 1_000 * DAY);
    assert!(said.contains("Nothing arranged"));
    assert!(said.contains("no envelope"), "{said}");
    assert!(said.contains("including the recovery codes"), "{said}");
}

#[test]
fn telling_the_same_person_twice_replaces_what_they_were_told() {
    // Two different answers about where the envelope is, held for one person,
    // is worse than none: whichever they act on, half the time it's wrong.
    let mut a = arranged();
    // Handed in as agreed, which is the case that matters: they agreed to a
    // sentence, and this is a different sentence.
    let mut moved = told("sam", true);
    moved.location = "the lockbox in the garage".into();
    a.tell(moved);

    assert_eq!(a.instructions.len(), 1, "{:#?}", a.instructions);
    assert!(a.instructions[0].location.contains("garage"));
    // And it is an un-agreed instruction again, because what they agreed to
    // is not what it now says.
    assert!(
        !a.instructions[0].they_know,
        "somebody's yes was carried across a move of the envelope"
    );

    // Re-stating the same thing is not a new agreement to get.
    let mut b = arranged();
    b.tell(told("Sam", false));
    assert!(b.instructions[0].they_know, "agreeing again to the identical sentence");
    assert!(a.gaps(&AfterMeConfig::default()).iter().any(|g| g.what.contains("hasn't been told")));

    // Agreement is recorded against the name, however it was capitalised.
    assert!(a.they_agreed("SAM"));
    assert!(a.instructions[0].they_know);
    assert!(!a.they_agreed("someone else"), "agreeing for a stranger");
}

// ================= review_every_days, which was the dead setting =================

#[test]
fn how_often_it_asks_you_to_check_the_arrangement_is_the_number_you_set() {
    let mut cfg = AfterMeConfig { enabled: true, review_every_days: 365, ..Default::default() };
    let a = Arrangement { reviewed_at: 100 * DAY, ..arranged() };

    assert!(!a.due_for_review(&cfg, 100 * DAY + 364 * DAY));
    assert!(a.due_for_review(&cfg, 100 * DAY + 365 * DAY));

    // The number is read, rather than a literal year that happens to equal it.
    cfg.review_every_days = 90;
    assert!(a.due_for_review(&cfg, 100 * DAY + 90 * DAY), "the interval is hardcoded");
    cfg.review_every_days = 3650;
    assert!(!a.due_for_review(&cfg, 100 * DAY + 365 * DAY));

    // Off means off, and zero means never.
    cfg.review_every_days = 365;
    cfg.enabled = false;
    assert!(!a.due_for_review(&cfg, 10_000 * DAY));
    cfg.enabled = true;
    cfg.review_every_days = 0;
    assert!(!a.due_for_review(&cfg, 10_000 * DAY));

    // Never reviewed is due. A clock that moved backwards is due rather than
    // locked out until real time catches up.
    cfg.review_every_days = 365;
    assert!(Arrangement { reviewed_at: 0, ..arranged() }.due_for_review(&cfg, 100 * DAY));
    assert!(Arrangement { reviewed_at: 200 * DAY, ..arranged() }.due_for_review(&cfg, 100 * DAY));
}

#[test]
fn it_does_not_nag_about_reviewing_an_arrangement_that_does_not_exist() {
    // Nothing arranged is a gap, and `gaps` is what says so. A yearly "check
    // your arrangement still holds" for an arrangement nobody made is how a
    // nudge gets ignored — and then it is ignored on the year it matters.
    let cfg = AfterMeConfig { enabled: true, review_every_days: 365, ..Default::default() };
    let nothing = Arrangement::default();
    assert!(!nothing.due_for_review(&cfg, 10_000 * DAY));
    assert!(nothing.nudge(&cfg, 10_000 * DAY).is_none());
}

#[test]
fn the_nudge_names_who_knows_and_how_to_answer_it() {
    let cfg = AfterMeConfig { enabled: true, review_every_days: 365, ..Default::default() };
    let said = arranged().nudge(&cfg, 10_000 * DAY).expect("overdue");
    assert!(said.contains("1 year"), "{said}");
    assert!(said.contains("Sam knows"), "{said}");
    assert!(said.contains("atlas afterme reviewed"), "{said}");

    // An arrangement with nobody told says that, rather than an empty list.
    let alone = Arrangement { instructions: Vec::new(), ..arranged() };
    let said = alone.nudge(&cfg, 10_000 * DAY).expect("overdue");
    assert!(said.contains("nobody has been told"), "{said}");
}

// ================= it is actually reachable =================

#[test]
fn the_command_and_the_daemon_are_what_reach_it_rather_than_these_tests() {
    // The point of the whole change. `afterme.rs` passed its tests for weeks
    // while nothing in the running program could call any of it.
    let main = crate::common::source_of("main");
    assert!(main.contains("fn run_afterme("), "there is no way in from the command line");
    assert!(main.contains("Some(\"afterme\")"), "and nothing dispatches to it");
    assert!(main.contains("atlas afterme"), "and the usage text doesn't mention it");

    let daemon = crate::common::source_of("daemon");
    assert!(
        daemon.contains("arrangement.nudge(&acfg, t)"),
        "review_every_days is a cadence nothing runs on"
    );
    assert!(
        daemon.contains("crate::afterme::RECORD"),
        "the daemon has no arrangement to read"
    );

    // And the section is no longer listed as parsed-and-never-read.
    let named: Vec<&str> = atlas::config::PARSED_AND_NEVER_READ.iter().map(|(k, _)| *k).collect();
    assert!(!named.contains(&"after_me"), "still listed as doing nothing: {named:?}");

    // The shipped file still offers it, still off.
    let raw = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");
    assert!(raw.contains("review_every_days:"));
    assert!(!AfterMeConfig::default().enabled, "it must ship off");
}

#[test]
fn what_somebody_typed_becomes_a_place_or_an_honest_no() {
    assert_eq!(afterme::where_from("yours"), Some(Where::YoursAlone));
    assert_eq!(afterme::where_from("BANK"), Some(Where::BankBox));
    assert_eq!(afterme::where_from("split"), Some(Where::SplitPhysically));
    assert_eq!(afterme::where_from("the safe"), None, "a guess is worse than a question");

    assert_eq!(afterme::timer_from("platforms"), Some(Timer::ThePlatforms));
    assert_eq!(afterme::timer_from("atlas"), Some(Timer::Atlas));
    assert_eq!(afterme::timer_from("whenever"), None);

    // And choosing Atlas as the timer is still told it is the wrong answer.
    let on_the_laptop = Arrangement { timer: Some(Timer::Atlas), ..arranged() };
    assert!(on_the_laptop
        .gaps(&AfterMeConfig::default())
        .iter()
        .any(|g| g.what.contains("count runs on the laptop")));
}
