//! One hello, said to someone who's there, and a brief with your own push
//! in it (1 Oct 2026, "why Atlas feels stale", ideas 1 and 4).

use atlas::brief;
use atlas::returning::{empty_hello, hello_now, Hello, GREET_GAP_SECS, GREET_HERE_WITHIN_SECS};

#[test]
fn nobody_at_the_desk_holds_the_hello() {
    assert_eq!(hello_now(0, 100_000, GREET_HERE_WITHIN_SECS), Hello::Hold);
    assert_eq!(hello_now(0, 100_000, 3600 * 8), Hello::Hold);
}

#[test]
fn a_hello_already_said_spends_the_next_one() {
    let welcomed = 100_000;
    assert_eq!(hello_now(welcomed, welcomed + 600, 5), Hello::Spent);
    assert_eq!(hello_now(welcomed, welcomed + GREET_GAP_SECS, 5), Hello::Say);
    assert_eq!(hello_now(0, welcomed, 5), Hello::Say);
}

#[test]
fn an_empty_hello_is_said_once_as_an_offer_and_then_not_at_all() {
    let first = empty_hello("Good evening", false, false).unwrap();
    assert!(first.starts_with("Good evening. "));
    assert!(first.contains("get to know me"));
    assert_eq!(empty_hello("Good evening", false, true), None);
    assert_eq!(empty_hello("Good evening", true, false), None);
}

#[test]
fn the_push_is_one_piece_a_day_in_turn() {
    let asked = "Push them on: posting, habits and deadlines";
    assert_eq!(brief::push_for_day(asked, 0).unwrap(), "You asked me to push you on posting -- what's today's step?");
    assert_eq!(brief::push_for_day(asked, 1).unwrap(), "You asked me to push you on habits -- what's today's step?");
    assert_eq!(brief::push_for_day(asked, 5).unwrap(), "You asked me to push you on deadlines -- what's today's step?");
    assert_eq!(brief::push_for_day("Push them on:  ", 0), None);
}

#[test]
fn a_brief_with_only_your_push_is_still_worth_saying() {
    let mut b = brief::run(&[], &[], &brief::BriefConfig::default());
    assert!(b.is_empty());
    b.push = brief::push_for_day("posting", 0);
    assert!(!b.is_empty());
    assert_eq!(brief::spoken(&b), "You asked me to push you on posting -- what's today's step?");
}

#[test]
fn the_part_of_day_greeted_survives_a_restart() {
    use atlas::nudge::{NudgeConfig, Nudger, Part};
    let mut n = Nudger::new(NudgeConfig::default());
    assert_eq!(n.last_daypart(), None);
    let t = 20_000 * 86_400 + 19 * 3600;
    let kept = Some((atlas::localclock::day_here(t) as u64, Part::Evening));
    let saved = serde_json::to_string(&kept).unwrap();
    n.set_last_daypart(serde_json::from_str(&saved).unwrap());
    assert_eq!(n.last_daypart(), kept);
    // Greeted already this evening: considering again says nothing.
    let again = n.consider(t, 19, 0, 0);
    assert!(again.map_or(true, |x| x.trigger != atlas::nudge::Trigger::Daypart));
}

/// Eric's own answers to "get to know me", 1 Oct 2026, read back as they
/// were: "You're working on I have two projects", "I'll push you on I need
/// you to push me on my habits".
#[test]
fn erics_own_answers_are_read_back_turned_round_and_short() {
    use atlas::getknow::{facts_from, Slot};
    let push = "I need you to push me on my habits, and my projects. I need you to take work off my plate where you can so looking at my projects and trying to further them";
    let f = facts_from(Slot::Push, push, 0);
    assert!(f[0].1.starts_with("I'll push you on your habits, and your projects."), "{}", f[0].1);
    assert_eq!(f[0].1.split_whitespace().count() <= 30, true, "{}", f[0].1);
    // The brief takes one piece a day from what was stored then -- the
    // raw sentence on the laptop as well as the cleaned one.
    for stored in [format!("Push them on: {push}"), f[0].0.summary.clone()] {
        let today = brief::push_for_day(&stored, 0).unwrap();
        assert_eq!(today, "You asked me to push you on your habits -- what's today's step?");
        assert_eq!(brief::push_for_day(&stored, 1).unwrap(), "You asked me to push you on your projects -- what's today's step?");
    }
    let day = facts_from(Slot::Day, "I have a day job, my schedule shifts a lot, I work 7AM-5PM most days", 0);
    assert_eq!(day[0].1, "Your day: you have a day job, your schedule shifts a lot, you work 7AM-5PM most days.");
    let work = facts_from(Slot::Work, "the bakery, my YouTube channel and Atlas", 0);
    assert_eq!(work.iter().map(|w| w.1.as_str()).collect::<Vec<_>>(), vec!["You're working on the bakery.", "You're working on your YouTube channel.", "You're working on Atlas."]);
}

/// The seventh question fills the opportunity hunt's two lists, so it has
/// something of yours to look for (why-stale idea 8).
#[test]
fn what_to_look_out_for_feeds_the_opportunity_hunt() {
    use atlas::getknow::{facts_from, Slot};
    let f = facts_from(Slot::Money, "I'm good at T-shirt design, video editing and selling on Etsy", 0);
    assert_eq!(f[0].1, "I'll look out for t-shirt design, video editing, selling on etsy.");
    let mut book = atlas::facts::Book::default();
    for (fact, _) in f {
        book.put(fact);
    }
    let you = atlas::hunt::Interests::from_facts(&book);
    assert_eq!(you.want, vec!["t-shirt design", "video editing", "selling on etsy"]);
    assert_eq!(you.skills, you.want);
}
