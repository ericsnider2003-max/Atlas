//! The line between your own work and a business you share.
//!
//! Eric's decision, implemented: block, notify, pause — and one way only.
//! Personal never reaches a business space; business freely reaches him.
//!
//! Most of these tests are about the ways a boundary quietly stops being one:
//! by being unsure and letting things through, by learning from being
//! overruled, by logging what it blocked, or by keeping the contents of the
//! thing it stopped.

use atlas::earned::Space;
use atlas::firewall::{note, Crossing, Firewall};

fn business(name: &str) -> Space {
    Space::Business(name.to_string())
}

// ---------------------------------------------------------------------------
// Which way it runs
// ---------------------------------------------------------------------------

#[test]
fn your_own_work_does_not_go_out() {
    let mut wall = Firewall::default();
    let verdict = wall.check(&Space::Personal, "Homelab", "tax-return.pdf", 100);
    assert!(!verdict.allowed(), "{verdict:?}");
    match verdict {
        Crossing::Stopped { held, why } => {
            assert!(held > 0, "it has to be held, not just refused");
            assert!(why.contains("your own work"), "{why}");
            assert!(why.contains("Homelab"), "and where it was going: {why}");
        }
        Crossing::Allowed => unreachable!(),
    }
}

#[test]
fn a_business_own_material_crosses_into_its_own_space_freely() {
    let mut wall = Firewall::default();
    assert!(wall.check(&business("Homelab"), "Homelab", "signals.csv", 100).allowed());
    assert!(wall.waiting().is_empty(), "nothing was stopped, so nothing is held");
}

#[test]
fn one_business_cannot_reach_another() {
    let mut wall = Firewall::default();
    let verdict = wall.check(&business("Homelab"), "Someone Else", "book.json", 100);
    assert!(!verdict.allowed());
    assert!(
        wall.waiting()[0].why.contains("belongs to Homelab"),
        "{:?}",
        wall.waiting()[0]
    );
}

#[test]
fn the_space_name_is_matched_without_fussing_about_capitals() {
    let mut wall = Firewall::default();
    assert!(wall.check(&business("Homelab"), "  homelab ", "signals.csv", 100).allowed());
}

// ---------------------------------------------------------------------------
// Unsure is not allowed
// ---------------------------------------------------------------------------

#[test]
fn nowhere_in_particular_is_stopped_rather_than_waved_through() {
    // An empty destination is a caller bug, and treating it as fine would be
    // the quietest possible hole in this.
    let mut wall = Firewall::default();
    assert!(!wall.check(&Space::Personal, "   ", "notes.md", 100).allowed());
    assert!(!wall.check(&business("Homelab"), "", "signals.csv", 100).allowed());
}

// ---------------------------------------------------------------------------
// Block, notify, pause — three things
// ---------------------------------------------------------------------------

#[test]
fn something_stopped_is_held_rather_than_lost() {
    // Blocking without holding means the work is simply gone, has to be
    // noticed, and has to be redone.
    let mut wall = Firewall::default();
    wall.check(&Space::Personal, "Homelab", "tax-return.pdf", 100);
    let waiting = wall.waiting();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].what, "tax-return.pdf");
    assert_eq!(waiting[0].into, "Homelab");
    assert!(!waiting[0].released);
}

#[test]
fn he_is_told_and_the_telling_does_not_read_it_out() {
    // A notification about a boundary that names what it stopped, on a screen
    // in front of the person it was stopped from, has defeated itself.
    let mut wall = Firewall::default();
    wall.check(&Space::Personal, "Homelab", "tax-return.pdf", 100);
    let n = note(wall.waiting()[0]);
    assert!(n.private, "it has to knock rather than disclose");
    assert!(n.body.contains("tax-return.pdf"), "{}", n.body);
    assert!(n.body.contains("release"), "and say what to do about it: {}", n.body);
    assert!(!n.title.contains("tax-return"), "the title is what a stranger sees: {}", n.title);
}

#[test]
fn releasing_one_thing_releases_exactly_that_thing() {
    // A firewall that learns from being overruled is one that eventually
    // stops refusing.
    let mut wall = Firewall::default();
    wall.check(&Space::Personal, "Homelab", "tax-return.pdf", 100);
    let first = wall.waiting()[0].id;
    assert!(wall.release(first).is_ok());
    assert!(wall.waiting().is_empty());

    // The same kind of thing, again. Still stopped.
    let again = wall.check(&Space::Personal, "Homelab", "tax-return.pdf", 200);
    assert!(!again.allowed(), "the rule did not loosen: {again:?}");
}

#[test]
fn releasing_the_same_thing_twice_is_said_rather_than_silently_repeated() {
    let mut wall = Firewall::default();
    wall.check(&Space::Personal, "Homelab", "notes.md", 100);
    let id = wall.waiting()[0].id;
    wall.release(id).unwrap();
    assert!(wall.release(id).is_err());
}

#[test]
fn dropping_something_removes_it_altogether() {
    let mut wall = Firewall::default();
    wall.check(&Space::Personal, "Homelab", "notes.md", 100);
    let id = wall.waiting()[0].id;
    assert!(wall.forget(id).is_ok());
    assert!(wall.get(id).is_none());
    assert!(wall.forget(id).is_err(), "and it stays gone");
}

#[test]
fn a_number_that_was_never_held_is_refused_by_name() {
    let mut wall = Firewall::default();
    assert!(wall.release(99).unwrap_err().contains("99"));
    assert!(wall.forget(99).unwrap_err().contains("99"));
}

// ---------------------------------------------------------------------------
// What is never written down
// ---------------------------------------------------------------------------

#[test]
fn the_held_list_keeps_a_name_and_never_a_path() {
    // The folders above a file are themselves personal. "Tax/2025/settlement"
    // says a great deal more than "settlement".
    let mut wall = Firewall::default();
    wall.check(&Space::Personal, "Homelab", "C:/Users/erics/Tax/2025/settlement.pdf", 100);
    let held = wall.waiting()[0];
    assert_eq!(held.what, "settlement.pdf");
    assert!(!held.what.contains("Tax"), "{}", held.what);
    assert!(!held.what.contains("erics"), "{}", held.what);
}

#[test]
fn contents_pasted_in_by_mistake_are_cut_rather_than_stored() {
    // The one thing this file must never hold is the thing it blocked.
    let mut wall = Firewall::default();
    let whole_letter = "Dear Sir, further to our correspondence of the fourteenth regarding the \
                        outstanding settlement and the associated schedule of payments";
    wall.check(&Space::Personal, "Homelab", whole_letter, 100);
    let held = wall.waiting()[0];
    assert!(held.what.chars().count() <= 61, "{} chars", held.what.chars().count());
    assert!(held.what.ends_with('…'), "{}", held.what);
}

#[test]
fn something_with_no_name_is_still_held_and_still_named_something() {
    let mut wall = Firewall::default();
    wall.check(&Space::Personal, "Homelab", "   ", 100);
    assert_eq!(wall.waiting()[0].what, "something with no name");
}

#[test]
fn nothing_in_here_stores_what_was_in_the_thing() {
    // Guarded by reading the source, because the failure is a field that does
    // not exist yet: the moment `Held` gains a body, the firewall's own log
    // has carried the content across the boundary it exists to hold.
    let source = include_str!("../src/firewall.rs");
    for field in ["pub body:", "pub contents:", "pub text:", "pub bytes:", "pub path:"] {
        assert!(!source.contains(field), "firewall must not keep {field}");
    }
    assert!(
        source.contains("never what was in it"),
        "the rule has to stay written where the next person will read it"
    );
}

// ---------------------------------------------------------------------------
// Saying it
// ---------------------------------------------------------------------------

#[test]
fn an_empty_line_says_so_plainly_rather_than_reporting_a_count_of_zero() {
    let said = Firewall::default().spoken();
    assert!(said.contains("Nothing's waiting"), "{said}");
    assert!(!said.contains('0'), "a count of zero is not an answer: {said}");
}

#[test]
fn one_thing_waiting_is_named_and_several_are_counted() {
    let mut wall = Firewall::default();
    wall.check(&Space::Personal, "Homelab", "notes.md", 100);
    assert!(wall.spoken().contains("notes.md"), "{}", wall.spoken());

    wall.check(&Space::Personal, "Homelab", "budget.xlsx", 200);
    let said = wall.spoken();
    assert!(said.contains('2'), "{said}");
    assert!(said.contains("budget.xlsx"), "the most recent is named: {said}");
}

#[test]
fn the_way_in_still_exists() {
    let main: &str = &crate::common::source_of("main");
    let shared_task = include_str!("../src/shared_task.rs");
    assert!(main.contains("Some(\"shared\")"), "nothing dispatches `atlas shared`");
    assert!(main.contains("atlas::firewall::Firewall::load"), "the held list is never read");

    // The boundary is asked where something actually crosses, which is
    // `shared_task::share_into_business`. It used to be asserted against
    // `wall.check(` in main.rs — and the only `wall.check(` in main.rs was
    // `atlas shared check`, the DRY RUN, which called the enforcement path
    // and then printed "Nothing has actually moved" while saving a real hold.
    // So this guard was satisfied by the one call site that should not have
    // been making it.
    assert!(
        shared_task.contains("firewall.check("),
        "nothing asks the boundary on a real crossing"
    );
    assert!(
        main.contains("wall.would_stop("),
        "`atlas shared check` is back to calling the enforcement path, so asking a \
         question creates and saves a hold"
    );

    // And notify, on the crossing that actually happened.
    //
    // `firewall::note`'s only caller was the dry run: Atlas built the
    // notification for a crossing that never occurred and built nothing for
    // the one that did.
    let notify_at = main.find("atlas::firewall::note(").expect(
        "block and pause are wired but notify isn't — nothing builds the notification, so \
         the one thing Eric ruled must happen alongside stopping something has nothing to \
         check it against",
    );
    let check_arm = main.find("wall.would_stop(").unwrap_or(0);
    let share_arm = main.find("tasks.share_into_business(").unwrap_or(usize::MAX);
    assert!(
        notify_at > share_arm.min(check_arm),
        "the notification is still built somewhere other than the real share"
    );
}

// ===================== asking is not doing ==============================
//
// `atlas shared check` printed "Held as {id}. Nothing has actually moved."
// and then called `wall.save(&store)`. The sentence is true about the file
// and false about the wall: `Firewall::check` is the enforcement path — it
// takes `&mut self`, allocates an id and pushes a `Held` — so every
// invocation of a subcommand named *check* created a real hold and wrote it
// to disk. Asking the same question three times left three entries in
// `atlas shared list`, each with an id you could "release".

#[test]
fn asking_whether_something_would_cross_holds_nothing() {
    // `&self`, not `&mut self`, and the compiler agrees — which is the
    // shortest statement of the fix there is.
    let wall = Firewall::default();
    let before = wall.waiting().len();

    for _ in 0..3 {
        let why = wall.would_stop(&Space::Personal, "acme");
        assert!(why.is_some(), "personal work into a business space should stop");
    }

    assert_eq!(
        wall.waiting().len(),
        before,
        "asking three times created {} holds — the question is not the act",
        wall.waiting().len() - before
    );
}

#[test]
fn the_answer_says_the_same_thing_the_act_would() {
    // The two halves live in one function so they cannot drift: `check` is
    // `would_stop` plus the recording. If they ever disagree, the dry run
    // stops predicting the real thing, which is the only reason to have one.
    let mut wall = Firewall::default();
    let predicted = wall.would_stop(&Space::Personal, "acme").expect("it would stop");
    match wall.check(&Space::Personal, "acme", "notes.txt", 1000) {
        Crossing::Stopped { why, held } => {
            assert_eq!(why, predicted, "the dry run and the real thing gave different reasons");
            assert!(held > 0, "a real crossing has an id you can release");
        }
        o => panic!("the real thing allowed what the dry run said would stop: {o:?}"),
    }
}

#[test]
fn a_businesss_own_work_is_allowed_by_both() {
    let mut wall = Firewall::default();
    assert!(wall.would_stop(&Space::Business("acme".into()), "acme").is_none());
    assert!(matches!(
        wall.check(&Space::Business("acme".into()), "acme", "invoice.pdf", 1000),
        Crossing::Allowed
    ));
    assert!(wall.waiting().is_empty(), "an allowed crossing was recorded as held");
}

#[test]
fn a_crossing_that_really_happens_is_still_recorded() {
    // So the fix cannot be satisfied by never holding anything.
    let mut wall = Firewall::default();
    let Crossing::Stopped { held, .. } = wall.check(&Space::Personal, "acme", "notes.txt", 1000)
    else {
        panic!("a real crossing was allowed");
    };
    assert_eq!(wall.waiting().len(), 1);
    assert!(wall.get(held).is_some(), "the hold cannot be found by the id it reported");
}

#[test]
fn what_it_would_say_names_the_thing_without_an_id_it_does_not_have() {
    // The third leg — block, pause, notify — shown for a crossing that did
    // not happen. Real text rather than a stub, and no hold behind it.
    let wall = Firewall::default();
    let why = wall.would_stop(&Space::Personal, "acme").expect("stops");
    let n = wall.would_say("acme", "Tax/2025/settlement-letter.pdf", &why, 1000);
    assert!(n.body.contains("settlement-letter.pdf"), "it did not name the thing: {}", n.body);
    assert!(
        !n.body.contains("Tax/2025"),
        "the folders above a file are themselves personal information: {}",
        n.body
    );
    assert!(n.body.contains("acme"), "it did not say where it was going: {}", n.body);
}
