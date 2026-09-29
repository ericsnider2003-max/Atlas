//! Confidence Atlas has earned.
//!
//! Eric's requirement, close to his words: the fix for "Atlas isn't confident
//! enough" must not be "raise the bar until it acts less". That is not
//! smarter, it is more restrictive, and it ends with him doing everything
//! himself. So the bar moves *down* as Atlas earns it, per kind of work, and
//! Atlas can say exactly what would move it further.

use atlas::earned::{kind_of, Kind, Record, Rope, Space, ENOUGH, WINDOW};
use atlas::intent::Intent;
use atlas::store::Store;
use std::path::PathBuf;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-earned-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn run(r: &mut Record, kind: Kind, good: usize, bad: usize) {
    for i in 0..good {
        r.note(kind, true, "fine", i as u64);
    }
    for i in 0..bad {
        r.note(kind, false, "wrong", (good + i) as u64);
    }
}

// ---------------------------------------------------------------------------
// Earning it
// ---------------------------------------------------------------------------

#[test]
fn a_new_atlas_asks_first_about_everything() {
    let r = Record::default();
    for k in Kind::all() {
        assert_eq!(r.rope(k), Rope::AskFirst, "{}", k.title());
        assert!(!r.may_act_alone(k));
    }
}

#[test]
fn a_short_run_of_luck_is_not_a_record() {
    let mut r = Record::default();
    run(&mut r, Kind::Housekeeping, ENOUGH - 1, 0);
    assert_eq!(
        r.rope(Kind::Housekeeping),
        Rope::AskFirst,
        "three right answers in a row happens by chance often enough that \
         acting on it would be acting on nothing"
    );
    run(&mut r, Kind::Housekeeping, 1, 0);
    assert_ne!(r.rope(Kind::Housekeeping), Rope::AskFirst, "now it is a record");
}

#[test]
fn getting_it_right_repeatedly_buys_more_rope_not_less() {
    let mut r = Record::default();
    run(&mut r, Kind::Housekeeping, 8, 2);
    let some = r.rope(Kind::Housekeeping);
    let mut r2 = Record::default();
    run(&mut r2, Kind::Housekeeping, 20, 0);
    assert!(
        r2.rope(Kind::Housekeeping) > some,
        "a clean record has to buy something or there is no point keeping one"
    );
    assert_eq!(r2.rope(Kind::Housekeeping), Rope::JustDo);
}

#[test]
fn two_recent_mistakes_pull_it_back_whatever_the_long_record_says() {
    let mut r = Record::default();
    run(&mut r, Kind::Housekeeping, 28, 0);
    assert_eq!(r.rope(Kind::Housekeeping), Rope::JustDo);
    run(&mut r, Kind::Housekeeping, 0, 2);
    assert_eq!(
        r.rope(Kind::Housekeeping),
        Rope::AskFirst,
        "falling has to be faster than rising, or the record is a counter"
    );
}

#[test]
fn one_mistake_is_not_a_pattern() {
    let mut r = Record::default();
    run(&mut r, Kind::Housekeeping, 20, 0);
    r.note(Kind::Housekeeping, false, "one bad one", 99);
    assert_ne!(
        r.rope(Kind::Housekeeping),
        Rope::AskFirst,
        "backing off on a single mistake is the restrictive fix in disguise"
    );
}

#[test]
fn a_good_record_in_one_kind_of_work_does_not_licence_another() {
    let mut r = Record::default();
    run(&mut r, Kind::Finding, 30, 0);
    assert_eq!(r.rope(Kind::Finding), Rope::JustDo);
    assert_eq!(
        r.rope(Kind::ReachingOut),
        Rope::AskFirst,
        "being excellent at reading files must not raise its licence to send \
         something to somebody"
    );
}

#[test]
fn money_and_secrets_can_never_be_earned() {
    let mut r = Record::default();
    run(&mut r, Kind::Sensitive, WINDOW, 0);
    assert_eq!(
        r.rope(Kind::Sensitive),
        Rope::AskFirst,
        "a perfect run must not buy the right to act alone here — that is \
         Eric's to give, not Atlas's to earn"
    );
    assert!(r.what_would_earn_more(Kind::Sensitive).contains("yours to decide"));
}

#[test]
fn reaching_another_person_is_capped_below_the_top() {
    let mut r = Record::default();
    run(&mut r, Kind::ReachingOut, WINDOW, 0);
    assert_eq!(
        r.rope(Kind::ReachingOut),
        Rope::DoAndSay,
        "a wrong confident guess here costs somebody who never agreed to the \
         risk, so it always says what it did"
    );
}

// ---------------------------------------------------------------------------
// Saying what would change it
// ---------------------------------------------------------------------------

#[test]
fn it_says_what_would_earn_more_rather_than_only_that_it_will_not() {
    let mut r = Record::default();
    let nothing_yet = r.what_would_earn_more(Kind::Drafting);
    assert!(
        nothing_yet.contains(&ENOUGH.to_string()) || nothing_yet.contains("record"),
        "\"not confident enough\" with no reason is what makes a system feel \
         arbitrary: {nothing_yet}"
    );

    // Interleaved, not a bad run at the end — a mediocre record and a recent
    // slump are different problems and get different answers.
    for i in 0..10u64 {
        r.note(Kind::Drafting, i % 3 != 0, "mixed", i);
    }
    let mediocre = r.what_would_earn_more(Kind::Drafting);
    assert!(mediocre.contains("10"), "it names the actual tally: {mediocre}");
    assert!(
        !mediocre.contains("wrong."),
        "a mediocre record is not a slump: {mediocre}"
    );

    let mut good = Record::default();
    run(&mut good, Kind::Housekeeping, 30, 0);
    assert!(good.what_would_earn_more(Kind::Housekeeping).contains("Nothing"));
}

#[test]
fn a_bad_run_says_so_plainly_rather_than_going_quiet() {
    let mut r = Record::default();
    run(&mut r, Kind::Housekeeping, 20, 3);
    let said = r.what_would_earn_more(Kind::Housekeeping);
    assert!(said.contains("last 3 wrong"), "{said}");
}

#[test]
fn nothing_it_says_reads_like_a_variable_name() {
    let mut r = Record::default();
    run(&mut r, Kind::ChangingTheMachine, 12, 1);
    for k in Kind::all() {
        let said = r.what_would_earn_more(k);
        assert!(!said.contains('_') && !said.contains("::"), "{said}");
        assert!(!k.title().contains('_'));
        assert!(k.note().split_whitespace().count() >= 4);
    }
}

// ---------------------------------------------------------------------------
// Keeping the record
// ---------------------------------------------------------------------------

#[test]
fn taking_something_back_rewrites_it_rather_than_counting_it_twice() {
    let mut r = Record::default();
    run(&mut r, Kind::Housekeeping, 10, 0);
    let (good_before, total_before) = r.tally(Kind::Housekeeping);
    assert!(r.taken_back());
    let (good_after, total_after) = r.tally(Kind::Housekeeping);
    assert_eq!(total_after, total_before, "one mistake, one entry");
    assert_eq!(good_after, good_before - 1);
}

#[test]
fn taking_back_something_already_wrong_changes_nothing() {
    let mut r = Record::default();
    run(&mut r, Kind::Housekeeping, 0, 1);
    assert!(!r.taken_back(), "no double-counting a mistake already recorded");
}

#[test]
fn a_busy_kind_of_work_does_not_push_a_quiet_one_out_of_its_own_history() {
    let mut r = Record::default();
    run(&mut r, Kind::Finding, 10, 0);
    run(&mut r, Kind::Sensitive, WINDOW * 3, 0);
    assert_eq!(
        r.tally(Kind::Finding).1,
        10,
        "trimming globally would reset a quiet category to untrusted every \
         time a busy one had a long day"
    );
}

#[test]
fn the_window_keeps_the_recent_past_not_the_whole_of_it() {
    let mut r = Record::default();
    run(&mut r, Kind::Finding, WINDOW * 2, 0);
    assert_eq!(r.tally(Kind::Finding).1, WINDOW);
}

#[test]
fn a_bad_week_can_be_recovered_from() {
    let mut r = Record::default();
    run(&mut r, Kind::Housekeeping, 0, 10);
    assert_eq!(r.rope(Kind::Housekeeping), Rope::AskFirst);
    run(&mut r, Kind::Housekeeping, WINDOW, 0);
    assert_eq!(
        r.rope(Kind::Housekeeping),
        Rope::JustDo,
        "a total rather than a window would mean a bad week is never lived down"
    );
}

#[test]
fn the_record_survives_a_restart() {
    let store = Store::new(tmp("roundtrip"));
    let mut r = Record::load(&store);
    run(&mut r, Kind::Finding, 12, 1);
    r.save(&store).unwrap();
    assert_eq!(Record::load(&store).tally(Kind::Finding), (12, 13));
}

// ---------------------------------------------------------------------------
// Which kind of work something is
// ---------------------------------------------------------------------------

#[test]
fn asking_a_question_is_not_the_same_kind_of_act_as_signing_into_something() {
    assert_eq!(kind_of(&Intent::Ask("what time is it".into())), Kind::Answering);
    assert_eq!(kind_of(&Intent::SignIn("a site".into())), Kind::Sensitive);
    assert_eq!(kind_of(&Intent::Files("the budget".into())), Kind::Finding);
    assert_eq!(kind_of(&Intent::DraftPost("a post".into())), Kind::Drafting);
}

#[test]
fn every_kind_has_a_distinct_key_and_a_cost() {
    let mut keys: Vec<&str> = Kind::all().iter().map(|k| k.key()).collect();
    let n = keys.len();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(n, keys.len());

    let costs: Vec<u8> = Kind::all().iter().map(|k| k.cost_of_being_wrong()).collect();
    assert!(costs.iter().all(|c| (1..=5).contains(c)));
    assert!(
        Kind::Sensitive.cost_of_being_wrong() > Kind::Answering.cost_of_being_wrong(),
        "being wrong about money must cost more than being wrong about a fact"
    );
}

#[test]
fn the_standing_covers_every_kind_so_none_is_invisible() {
    let r = Record::default();
    assert_eq!(r.standing().len(), Kind::all().len());
}


// ---------------------------------------------------------------------------
// Graduation: personal work first, business work after
// ---------------------------------------------------------------------------

fn acme() -> Space {
    Space::Business("Acme".into())
}

fn run_in(r: &mut Record, space: &Space, kind: Kind, good: usize, bad: usize) {
    for i in 0..good {
        r.note_in(space, kind, true, "fine", i as u64);
    }
    for i in 0..bad {
        r.note_in(space, kind, false, "wrong", (good + i) as u64);
    }
}

#[test]
fn a_perfect_business_record_earns_nothing_while_the_personal_one_is_untested() {
    let mut r = Record::default();
    run_in(&mut r, &acme(), Kind::Drafting, WINDOW, 0);
    assert_eq!(
        r.rope_in(&acme(), Kind::Drafting),
        Rope::AskFirst,
        "otherwise a brand-new business is where Atlas quietly earns its first \
         licence, with the one person who would notice a wrong tone not looking"
    );
}

#[test]
fn the_reason_given_is_the_gate_not_the_business_record() {
    let mut r = Record::default();
    run_in(&mut r, &acme(), Kind::Drafting, WINDOW, 0);
    let said = r.what_would_earn_more_in(&acme(), Kind::Drafting);
    assert!(
        said.contains("your own work"),
        "when the gate is shut, nothing about the business's own record is the \
         reason: {said}"
    );
}

#[test]
fn being_trusted_personally_opens_the_gate_but_does_not_walk_through_it() {
    let mut r = Record::default();
    run(&mut r, Kind::Drafting, WINDOW, 0);
    assert_eq!(r.rope(Kind::Drafting), Rope::JustDo, "trusted on his own work");
    assert_eq!(
        r.rope_in(&acme(), Kind::Drafting),
        Rope::AskFirst,
        "the business has its own record to build, and it hasn't"
    );
}

#[test]
fn business_work_is_always_said_out_loud_however_good_both_records_are() {
    let mut r = Record::default();
    run(&mut r, Kind::Drafting, WINDOW, 0);
    run_in(&mut r, &acme(), Kind::Drafting, WINDOW, 0);
    assert_eq!(
        r.rope_in(&acme(), Kind::Drafting),
        Rope::DoAndSay,
        "the risk lands on somebody who never agreed to it, so this cap is not \
         earnable"
    );
    assert!(r.may_act_alone_in(&acme(), Kind::Drafting));
}

#[test]
fn business_work_never_outranks_the_same_work_done_personally() {
    let mut r = Record::default();
    // Good enough personally to act and say; excellent for the business.
    run(&mut r, Kind::Housekeeping, 9, 2);
    run_in(&mut r, &acme(), Kind::Housekeeping, WINDOW, 0);
    assert!(
        r.rope_in(&acme(), Kind::Housekeeping) <= r.rope(Kind::Housekeeping),
        "a business record must not be able to buy more rope than the work it \
         graduated from"
    );
}

#[test]
fn losing_trust_personally_pulls_the_business_back_with_it() {
    let mut r = Record::default();
    run(&mut r, Kind::Housekeeping, WINDOW, 0);
    run_in(&mut r, &acme(), Kind::Housekeeping, WINDOW, 0);
    assert_eq!(r.rope_in(&acme(), Kind::Housekeeping), Rope::DoAndSay);

    run(&mut r, Kind::Housekeeping, 0, 2);
    assert_eq!(
        r.rope_in(&acme(), Kind::Housekeeping),
        Rope::AskFirst,
        "if Atlas has started getting this wrong for him, it does not carry on \
         doing it unattended for a partner"
    );
}

#[test]
fn money_and_secrets_stay_shut_in_a_business_too() {
    let mut r = Record::default();
    run(&mut r, Kind::Sensitive, WINDOW, 0);
    run_in(&mut r, &acme(), Kind::Sensitive, WINDOW, 0);
    assert_eq!(r.rope_in(&acme(), Kind::Sensitive), Rope::AskFirst);
}

#[test]
fn two_businesses_keep_separate_records() {
    let other = Space::Business("Beta".into());
    let mut r = Record::default();
    run(&mut r, Kind::Drafting, WINDOW, 0);
    run_in(&mut r, &acme(), Kind::Drafting, WINDOW, 0);
    assert_eq!(r.rope_in(&acme(), Kind::Drafting), Rope::DoAndSay);
    assert_eq!(
        r.rope_in(&other, Kind::Drafting),
        Rope::AskFirst,
        "a record earned with one partner is not evidence about another"
    );
    assert_eq!(r.businesses(), vec!["Acme".to_string()]);
}

#[test]
fn a_business_does_not_eat_the_personal_history() {
    let mut r = Record::default();
    run(&mut r, Kind::Drafting, 10, 0);
    run_in(&mut r, &acme(), Kind::Drafting, WINDOW * 3, 0);
    assert_eq!(
        r.tally(Kind::Drafting).1,
        10,
        "trimming across spaces would let a busy business wipe the record it \
         is supposed to be graduating from"
    );
}

#[test]
fn a_business_is_named_the_way_you_named_it() {
    assert_eq!(acme().title(), "Acme");
    assert_eq!(Space::Personal.title(), "Your own work");
    assert!(acme().is_business());
    assert!(!Space::Personal.is_business());
}
