use atlas::person::{
    self, beyond_me, hard_day, learn_from_edit, learn_from_refusal, Kind, Noticed, Person,
    PersonConfig, Said, NOT_A_THERAPIST,
};

const DAY: u64 = 86_400;

/// Judgment settings for these tests.
///
/// `is_late` and `usual_hours` measure an hour against the spread of your own
/// hours now, rather than against a fraction of your busiest one — a single
/// heavy hour used to halve every other hour's share, so one 10am spike made
/// most of a working day read as "late".
fn jc() -> atlas::judgment::JudgmentConfig {
    atlas::judgment::JudgmentConfig::default()
}

// ================= learned from what happens, not a form =================

#[test]
fn one_observation_is_not_a_pattern() {
    let mut p = Person::default();
    p.notice("works late on Thursdays", Kind::Rhythm, 0);
    assert!(p.known(0).is_empty(), "once is a coincidence");
    p.notice("works late on Thursdays", Kind::Rhythm, DAY);
    p.notice("works late on Thursdays", Kind::Rhythm, 2 * DAY);
    assert_eq!(p.known(2 * DAY).len(), 1);
}

#[test]
fn an_old_pattern_confidently_applied_is_worse_than_none() {
    // People change how they work.
    let mut p = Person::default();
    for i in 0..4 {
        p.notice("starts with email", Kind::Rhythm, i * DAY);
    }
    assert_eq!(p.known(4 * DAY).len(), 1);
    assert!(p.known(60 * DAY).is_empty(), "six weeks on, it's not safe to assume");
}

#[test]
fn what_you_change_about_its_output_is_the_strongest_signal() {
    // It's you saying what you wanted, with an example attached.
    let long = "I wanted to reach out to see whether you might have a moment to consider \
                looking at the attached document at your earliest convenience.";
    let short = "Can you look at this?";
    let (what, kind) = learn_from_edit(long, short).unwrap();
    assert!(what.contains("cuts what I write down"));
    assert_eq!(kind, Kind::Correction);
}

#[test]
fn adding_detail_is_learned_as_readily_as_cutting_it() {
    let terse = "Done.";
    let fuller = "Done — the index finished at 4pm, 12,000 files, nothing failed.";
    assert!(learn_from_edit(terse, fuller).unwrap().0.contains("adds detail"));
}

#[test]
fn a_rewrite_for_tone_is_told_apart_from_a_trim() {
    let before = "We should probably consider possibly moving the deadline somewhat.";
    let after = "Push the deadline. Two weeks.";
    assert!(learn_from_edit(before, after).is_some());
}

#[test]
fn no_change_teaches_nothing() {
    assert!(learn_from_edit("same text", "same text").is_none());
}

#[test]
fn saying_no_is_learned_as_one_fact_not_a_rule() {
    let (what, kind) = learn_from_refusal("posting to LinkedIn without asking");
    assert!(what.starts_with("said no to:"));
    assert_eq!(kind, Kind::Taste);
}

#[test]
fn your_working_hours_are_learned_from_when_you_actually_work() {
    let mut p = Person::default();
    for h in [9, 10, 10, 11, 14, 15, 15, 16, 22] {
        p.worked_at(h);
    }
    let (from, to) = p.usual_hours(&jc()).unwrap();
    assert!(from <= 10 && to >= 15);
}

#[test]
fn atlas_will_not_guess_your_hours_from_two_data_points() {
    let mut p = Person::default();
    p.worked_at(9);
    p.worked_at(10);
    assert!(p.usual_hours(&jc()).is_none());
}

// ================= you can read it and argue =================

#[test]
fn everything_it_thinks_it_knows_can_be_read() {
    // A system that learns about you and won't show you what it learned is a
    // system you can't correct.
    let mut p = Person::default();
    for i in 0..3 {
        p.notice("prefers short answers", Kind::Taste, i * DAY);
    }
    let shown = p.show(3 * DAY);
    assert!(shown.contains("prefers short answers"));
    assert!(shown.contains("seen 3 times"), "with how sure it is");
    assert!(shown.contains("forget"), "and how to remove it");
    // Behaviour, not just wording: what is shown is the real stored state --
    // one trait, its count the true number of times it was noticed.
    let known = p.known(3 * DAY);
    assert_eq!(known.len(), 1, "more or fewer traits are known than are shown");
    assert_eq!(known[0].seen, 3, "the shown count does not match the stored one");
}

#[test]
fn forgetting_is_immediate_and_not_argued_with() {
    let mut p = Person::default();
    for i in 0..3 {
        p.notice("works late on Thursdays", Kind::Rhythm, i * DAY);
    }
    assert_eq!(p.forget("works late"), 1);
    assert!(p.known(3 * DAY).is_empty());
}

#[test]
fn nothing_learned_yet_says_so_rather_than_inventing_something() {
    assert!(Person::default().show(0).contains("haven't worked anything out"));
}

// ================= noticing without diagnosing =================

#[test]
fn everything_it_notices_is_checkable_against_a_clock() {
    // "You've been at this since six" is an observation. "You seem stressed"
    // is a guess dressed as insight, and a wrong one is worse than silence.
    let late = Noticed::LateRun { nights: 4 };
    assert!(late.spoken().contains("4 nights running past your usual hours"));
    assert!(!late.spoken().contains("stressed"));
    assert!(!late.spoken().contains("burn"));
}

#[test]
fn an_observation_comes_with_an_offer_not_with_advice() {
    assert!(Noticed::NoBreak { hours: 7 }.spoken().contains("if you want to stop"));
    assert!(Noticed::LateRun { nights: 3 }.spoken().contains("Want me to take anything off"));
    // Never "you should".
    for n in [
        Noticed::LateRun { nights: 3 },
        Noticed::NoBreak { hours: 7 },
        Noticed::Scattered { started: 9, finished: 2 },
    ] {
        assert!(!n.spoken().to_lowercase().contains("you should"), "{}", n.spoken());
    }
}

#[test]
fn a_stalled_project_is_asked_about_rather_than_assumed_to_be_forgotten() {
    let s = Noticed::Stalled { project: "Homelab".into(), weeks: 5 };
    assert!(s.spoken().contains("parked on purpose?"), "got: {}", s.spoken());
    assert!(s.say_once(), "and not raised every week after that");
}

#[test]
fn a_project_quiet_for_a_week_is_normal_and_not_mentioned() {
    let mut p = Person::default();
    p.touched("Atlas", 0);
    assert!(p.gone_quiet(7 * DAY).is_empty(), "a week is nothing");
    // How long, as well as what: "hasn't moved in 5 weeks" is worth saying
    // and "has gone quiet" is not.
    assert_eq!(p.gone_quiet(40 * DAY), vec![("Atlas", 5)]);
    assert!(p.gone_quiet(200 * DAY).is_empty(), "and after long enough, it's just over");
}

#[test]
fn the_defaults_wait_a_while_before_saying_anything() {
    let c = PersonConfig::default();
    assert!(c.late_nights_before_saying >= 3);
    assert!(c.hours_before_saying >= 6);
    assert!(c.quiet_days >= 7, "and doesn't repeat itself");
}

// ================= the line =================

#[test]
fn atlas_says_plainly_when_something_is_beyond_it() {
    assert!(NOT_A_THERAPIST.contains("not the right thing for this"));
    assert!(NOT_A_THERAPIST.contains("a person is better than me"));
    assert!(NOT_A_THERAPIST.contains("isn't me deflecting"), "and it doesn't just hang up");
    assert!(NOT_A_THERAPIST.contains("take things off your plate"), "it still offers what it can");
}

#[test]
fn the_line_is_narrow_on_purpose() {
    // Treating every bad week as a crisis is its own kind of unhelpful.
    assert!(!beyond_me("I've had an awful week"));
    assert!(!beyond_me("this project is killing me"));
    assert!(!beyond_me("I'm exhausted and behind on everything"));
    assert!(beyond_me("I don't think it's worth living anymore"));
}

#[test]
fn on_a_hard_day_it_offers_to_do_something_rather_than_to_talk() {
    // The useful thing here is almost never words.
    let said = hard_day(&["the nine o'clock post".into(), "the index".into()]);
    assert!(said.contains("I can deal with the nine o'clock post"));
    assert!(said.contains("1 other thing"));
    assert!(!said.contains("How does that make you feel"));
}

#[test]
fn with_nothing_to_take_on_it_says_little_and_does_not_perform_concern() {
    let said = hard_day(&[]);
    assert!(said.len() < 90, "short: {said}");
    assert!(said.contains("anything I can take off you"));
}

// ================= the producer Noticed never had =================
//
// Four variants, each with a sentence and a rule about repeating, and nothing
// in the tree ever built one. Three settings were thresholds on it —
// `notice_patterns`, `late_nights_before_saying`, `hours_before_saying` — and
// a fourth, `quiet_days`, decided how often the same remark could be made.

/// Somebody who usually works 9 to 5, established over enough days that
/// `usual_hours` will answer.
fn nine_to_five() -> Person {
    let mut p = Person::default();
    for _ in 0..5 {
        for h in 9..=17 {
            p.worked_at(h);
        }
    }
    p
}

#[test]
fn a_late_night_is_one_outside_the_hours_you_actually_keep() {
    let mut p = nine_to_five();
    let (from, to) = p.usual_hours(&jc()).expect("five days is enough to have hours");
    assert!(from <= 9 && to >= 17, "{from}-{to}");

    // Two in the morning, three nights running.
    let day = 86_400u64;
    for d in 1..=3u64 {
        p.working_now(2, d * day + 2 * 3600, &jc());
    }
    assert_eq!(p.late_run(3 * day + 2 * 3600), 3);
    // And it drifts, on purpose. A fourth night and 2am is no longer a late
    // hour — at four nights running you work nights, and a laptop that went
    // on remarking on it every evening would be wrong as well as annoying.
    p.working_now(2, 4 * day + 2 * 3600, &jc());
    assert_eq!(p.late_run(4 * day + 2 * 3600), 0, "it never stops calling you late");
    // Counted in nights rather than in sessions: twice in one night is one.
    p.working_now(3, 3 * day + 3 * 3600, &jc());
    assert_eq!(p.late_run(3 * day + 3 * 3600), 3, "two stretches in one night counted twice");

    // A gap breaks the run.
    let mut q = nine_to_five();
    q.working_now(2, 1 * day, &jc());
    q.working_now(2, 5 * day, &jc());
    assert_eq!(q.late_run(5 * day), 1, "a run with a gap in it is not a run");

    // An hour inside your usual range is not a late night, however many times.
    let mut r = nine_to_five();
    for d in 1..=5u64 {
        r.working_now(14, d * day, &jc());
    }
    assert_eq!(r.late_run(5 * day), 0);
}

#[test]
fn how_many_nights_before_it_says_anything_is_the_number_you_set() {
    let day = 86_400u64;
    let mut p = nine_to_five();
    for d in 1..=3u64 {
        p.working_now(2, d * day, &jc());
    }
    let now = 3 * day;

    let mut cfg = PersonConfig::default();
    assert_eq!(cfg.late_nights_before_saying, 3, "the shipped default");
    assert_eq!(
        person::noticing(&p, &cfg, 0, now),
        Some(Noticed::LateRun { nights: 3 })
    );

    // The number is read, not a literal three that happens to equal it.
    cfg.late_nights_before_saying = 10;
    assert!(!matches!(
        person::noticing(&p, &cfg, 0, now),
        Some(Noticed::LateRun { .. })
    ));
    // Zero is never, rather than every time.
    cfg.late_nights_before_saying = 0;
    assert!(!matches!(
        person::noticing(&p, &cfg, 0, now),
        Some(Noticed::LateRun { .. })
    ));

    // And both switches above it still mean off.
    cfg.late_nights_before_saying = 3;
    cfg.notice_patterns = false;
    assert_eq!(person::noticing(&p, &cfg, 99, now), None);
    cfg.notice_patterns = true;
    cfg.enabled = false;
    assert_eq!(person::noticing(&p, &cfg, 99, now), None);
}

#[test]
fn how_long_at_it_before_it_says_anything_is_also_yours() {
    let cfg = PersonConfig::default();
    let p = Person::default();
    assert_eq!(cfg.hours_before_saying, 6);
    assert_eq!(person::noticing(&p, &cfg, 5, 0), None);
    assert_eq!(person::noticing(&p, &cfg, 6, 0), Some(Noticed::NoBreak { hours: 6 }));

    let mut longer = PersonConfig::default();
    longer.hours_before_saying = 12;
    assert_eq!(person::noticing(&p, &longer, 6, 0), None, "the threshold is hardcoded");
    assert_eq!(person::noticing(&p, &longer, 12, 0), Some(Noticed::NoBreak { hours: 12 }));

    let mut never = PersonConfig::default();
    never.hours_before_saying = 0;
    assert_eq!(person::noticing(&p, &never, 99, 0), None, "zero must mean never");
}

#[test]
fn it_says_one_thing_and_the_most_pressing_one() {
    // Four observations at once is a lecture, and nobody takes a lecture from
    // their laptop.
    let day = 86_400u64;
    let mut p = nine_to_five();
    for d in 1..=3u64 {
        p.working_now(2, d * day, &jc());
    }
    p.touched("the tax return", 0);
    // Late nights and a stalled project both apply; the late run leads.
    assert!(matches!(
        person::noticing(&p, &PersonConfig::default(), 9, 3 * day),
        Some(Noticed::LateRun { .. })
    ));
    let now = 40 * day;

    // With no late nights, the stalled project is what gets said, and it says
    // how long rather than just that it has gone quiet.
    let mut quiet = Person::default();
    quiet.touched("the tax return", 0);
    match person::noticing(&quiet, &PersonConfig::default(), 0, now) {
        Some(Noticed::Stalled { project, weeks }) => {
            assert_eq!(project, "the tax return");
            assert!(weeks >= 5, "{weeks}");
        }
        other => panic!("nothing said about a project still since February: {other:?}"),
    }
}

#[test]
fn how_often_it_can_repeat_itself_is_the_number_you_set() {
    let day = 86_400u64;
    let cfg = PersonConfig::default();
    assert_eq!(cfg.quiet_days, 7);
    let seen = Noticed::LateRun { nights: 3 };

    let mut said = Said::default();
    assert!(said.may_say(&seen, &cfg, 0), "it has never said anything");
    said.record(&seen, 0);
    assert!(!said.may_say(&seen, &cfg, 6 * day));
    assert!(said.may_say(&seen, &cfg, 7 * day));

    // The same remark with different numbers is the same remark. "3 nights
    // running" and "4 nights running" are not two observations.
    assert!(!said.may_say(&Noticed::LateRun { nights: 9 }, &cfg, day));
    // A different one is not silenced by it.
    assert!(said.may_say(&Noticed::NoBreak { hours: 9 }, &cfg, day));

    // The number is read.
    let mut chatty = PersonConfig::default();
    chatty.quiet_days = 1;
    assert!(said.may_say(&seen, &chatty, day), "the quiet period is hardcoded");

    // Some things are worth saying once and then dropping: a project has not
    // moved in a month, and saying so again next month adds nothing.
    let once = Noticed::Stalled { project: "the tax return".into(), weeks: 5 };
    assert!(once.say_once());
    let mut s2 = Said::default();
    assert!(s2.may_say(&once, &cfg, 0));
    s2.record(&once, 0);
    assert!(!s2.may_say(&once, &cfg, 1000 * day), "it came back");

    // A clock that moved backwards says it twice rather than never again.
    let mut s3 = Said::default();
    s3.record(&seen, 100 * day);
    assert!(s3.may_say(&seen, &cfg, 50 * day));
}

#[test]
fn the_daemon_is_what_notices_rather_than_these_tests() {
    // The whole point. All of the above passed for weeks while nothing in the
    // running program ever built a `Noticed`.
    let src = crate::common::source_of("daemon");
    assert!(
        src.contains("crate::person::noticing(&self.person, &pcfg, hours_straight, t)"),
        "nothing in the tick notices anything"
    );
    assert!(
        src.contains("self.person.working_now(hour, t, &self.tools_cfg().judgment)"),
        "nothing counts the late nights, or it counts them without your own hours to judge by"
    );
    assert!(
        src.contains("crate::person::SAID_RECORD"),
        "quiet_days has no memory of what was said, so it cannot mean anything"
    );
    assert!(
        src.contains("working_since"),
        "nothing counts the unbroken stretch hours_before_saying is a threshold on"
    );

    let raw = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");
    for key in ["notice_patterns:", "late_nights_before_saying:", "hours_before_saying:", "quiet_days:"] {
        assert!(raw.contains(key), "{key} is no longer in the shipped config");
    }
}
