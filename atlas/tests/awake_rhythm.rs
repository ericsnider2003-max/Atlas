use atlas::awake::{decide, AwakeConfig, Because, Hold, OnWaking, Power, WHAT_THIS_DOES};
use atlas::daily::{
    day_of_with, find_dropped, picking_back_up, DailyConfig, Dropped, Rhythm,
};

const DAY: u64 = 86_400;

// ================= it works out your day rather than asking =================

fn nine_to_six() -> Rhythm {
    let mut r = Rhythm::default();
    for _ in 0..20 {
        r.note_day();
        for h in 9..19 {
            for _ in 0..5 {
                r.saw(h, true);
            }
        }
    }
    r
}

#[test]
fn you_never_have_to_tell_it_when_you_work() {
    // Asking someone to declare their hours is asking them to maintain a
    // setting they'll get wrong twice a year.
    let r = nine_to_six();
    let hour = r.quiet_hour().expect("it should have worked this out");
    assert!(!(9..19).contains(&hour), "rolled at {hour}, which is during your day");
    assert!(r.noticed().unwrap().contains("nothing to set"));
    assert!(r.noticed().unwrap().contains("keeps adjusting"), "adaptive (F2)");
}

#[test]
fn atlas_working_overnight_is_not_you_working() {
    // A machine that ran a job at three in the morning has told you nothing
    // about your day.
    let mut r = nine_to_six();
    for _ in 0..200 {
        r.saw(3, false);
    }
    let hour = r.quiet_hour().unwrap();
    assert!(!(9..19).contains(&hour));
    assert_eq!(r.active_by_hour[3], 0, "Atlas's own work isn't counted");
}

#[test]
fn a_night_owl_gets_a_later_rollover_without_saying_so() {
    let mut r = Rhythm::default();
    for _ in 0..20 {
        r.note_day();
        for h in [14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 0, 1, 2] {
            for _ in 0..5 {
                r.saw(h, true);
            }
        }
    }
    let hour = r.quiet_hour().unwrap();
    assert!((3..13).contains(&hour), "rolled at {hour} for someone up until 2am");
}

#[test]
fn a_fortnight_before_it_trusts_itself() {
    // Less than that and one late night moves your whole day.
    let mut r = Rhythm::default();
    for _ in 0..5 {
        r.note_day();
        for h in 9..18 {
            r.saw(h, true);
        }
    }
    assert!(r.quiet_hour().is_none());
    assert_eq!(r.rolls_at(&DailyConfig::default()), 0, "falls back rather than guessing");
}

#[test]
fn the_rollover_hour_actually_shifts_the_day() {
    // Pinned here (28 Sep 2026): this is about a time of day, and it passed
    // only when cargo found `.cargo/config.toml` (run from the crate's folder).
    atlas::localclock::pin_offset(Some(0));
    assert_eq!(day_of_with(10 * DAY + 2 * 3600, 4), 9 * DAY);
    assert_eq!(day_of_with(10 * DAY + 10 * 3600, 4), 10 * DAY);
}

// ================= sleep =================

fn plugged_in() -> Power {
    Power { on_battery: false, battery_pct: 100, lid_closed: false,
            lid_action: atlas::awake::LidAction::Sleep, external_display: false }
}

#[test]
fn it_holds_the_machine_up_only_while_something_is_running() {
    let cfg = AwakeConfig::default();
    assert_eq!(decide(Because::YouAskedForIt, &plugged_in(), 5, &cfg).0, Hold::SystemOnly);
    assert_eq!(decide(Because::Nothing, &plugged_in(), 0, &cfg).0, Hold::Release);
}

#[test]
fn the_screen_never_stays_on_and_that_is_not_configurable() {
    // It's the whole battery cost and none of the benefit.
    //
    // This used to assert `!AwakeConfig::default().may_keep_screen_on` on a
    // `#[serde(skip)]` field pinned false that nothing read -- which is to
    // say it asserted that a constant was the constant it was declared as.
    // The field is gone (19 Sep 2026) and what is checked now is the thing
    // that actually makes the promise true: `Hold` has two variants and
    // neither of them is "and the screen", so `decide` cannot return one
    // however it is configured.
    let src = std::fs::read_to_string("src/awake.rs").expect("src/awake.rs");
    let body = src.split("pub enum Hold").nth(1).expect("Hold is gone");
    let body = &body[..body.find("\n}").expect("unterminated Hold")];
    let variants: Vec<&str> = body
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with("//") && !l.is_empty() && *l != "{")
        .map(|l| l.trim_end_matches(','))
        .collect();
    assert_eq!(
        variants,
        vec!["SystemOnly", "Release"],
        "`Hold` gained a variant. If one of them keeps the screen on, that is a battery \
         decision, not a refactor -- the whole point of this enum is that it cannot say it."
    );

    // And an old config naming the removed key still loads.
    let parsed: AwakeConfig =
        serde_yaml::from_str("enabled: true\nmay_keep_screen_on: true\n").unwrap();
    assert!(parsed.enabled, "an unknown key must not stop the section parsing");
}

#[test]
fn a_closed_lid_is_a_decision_you_made_and_is_not_overridden() {
    // How a laptop cooks in a bag.
    let shut = Power { lid_closed: true, ..plugged_in() };
    let (hold, why) = decide(Because::OvernightWork, &shut, 5, &AwakeConfig::default());
    assert_eq!(hold, Hold::Release);
    assert!(why.contains("running in a bag"));
}

#[test]
fn a_low_battery_wins_over_finishing_the_job() {
    let low = Power { on_battery: true, battery_pct: 12, ..plugged_in() };
    let (hold, why) = decide(Because::YouAskedForIt, &low, 5, &AwakeConfig::default());
    assert_eq!(hold, Hold::Release);
    assert!(why.contains("want that in the morning"));
}

#[test]
fn something_holding_the_machine_up_for_hours_is_stuck_not_busy() {
    let (hold, why) = decide(Because::OvernightWork, &plugged_in(), 200, &AwakeConfig::default());
    assert_eq!(hold, Hold::Release);
    assert!(why.contains("Something's stuck"));
}

#[test]
fn coming_back_correctly_matters_more_than_not_sleeping() {
    // The machine will sleep — you'll shut the lid, the battery will go,
    // Windows will update.
    assert!(WHAT_THIS_DOES.contains("coming back correctly afterwards"));
    assert!(WHAT_THIS_DOES.contains("I can't do is stop you shutting the lid"));
}

// ================= things you dropped =================

fn dropped() -> Vec<Dropped> {
    vec![
        Dropped {
            title: "rewrite the fee comparison".into(),
            when: 20 * DAY,
            carried_for: 9,
            about: Some("Content".into()),
            thinking: vec![],
        },
        Dropped {
            title: "look at the new broker tier".into(),
            when: 40 * DAY,
            carried_for: 1,
            about: Some("Homelab".into()),
            thinking: vec![],
        },
    ]
}

#[test]
fn dropped_things_are_kept_because_i_dropped_that_what_was_it_is_a_real_question() {
    let all = dropped();
    let found = find_dropped(&all, "the fee thing");
    assert_eq!(found.len(), 1);
    assert!(found[0].title.contains("fee comparison"));
}

#[test]
fn you_can_find_one_by_the_project_rather_than_the_words() {
    let all = dropped();
    let found = find_dropped(&all, "Homelab");
    assert_eq!(found[0].title, "look at the new broker tier");
}

#[test]
fn something_dropped_after_a_long_slog_says_so_when_it_comes_back() {
    // The reason it was dropped hasn't changed.
    let d = &dropped()[0];
    assert!(d.was_a_slog());
    let said = picking_back_up(d);
    assert!(said.contains("on the list 9 days"));
    assert!(said.contains("might want to be smaller this time"));
}

#[test]
fn something_dropped_quickly_gets_no_lecture() {
    let said = picking_back_up(&dropped()[1]);
    assert!(!said.contains("smaller this time"));
}

// ================= locked is not asleep =================

use atlas::awake::{
    on_waking_checked, running_state, woke_checked, Checked, LidAction, Running, BUDGET_MS,
};

fn on_a_stand() -> Power {
    Power {
        on_battery: false,
        battery_pct: 100,
        lid_closed: true,
        lid_action: LidAction::Nothing,
        external_display: true,
    }
}

#[test]
fn a_shut_lid_that_does_nothing_does_not_stop_atlas() {
    // Plenty of people run with the laptop shut on a stand. Stopping every
    // time they close it would be maddening.
    assert!(!LidAction::Nothing.stops_the_machine());
    assert!(!LidAction::ScreenOff.stops_the_machine());
    assert!(LidAction::Sleep.stops_the_machine());

    let (hold, _) = decide(Because::OvernightWork, &on_a_stand(), 5, &AwakeConfig::default());
    assert_eq!(hold, Hold::SystemOnly, "shut lid, set to nothing, still working");
}

#[test]
fn a_shut_lid_on_a_machine_that_sleeps_still_stops() {
    let sleeps = Power {
        lid_action: LidAction::Sleep,
        external_display: false,
        ..on_a_stand()
    };
    let (hold, why) = decide(Because::OvernightWork, &sleeps, 5, &AwakeConfig::default());
    assert_eq!(hold, Hold::Release);
    assert!(why.contains("this machine sleeps when it is"));
}

#[test]
fn locked_with_the_screens_off_is_not_sleep() {
    // They look identical from across the room and are completely different:
    // one is still working and one isn't.
    assert_eq!(running_state(&on_a_stand(), true, true), Running::LockedButUp);
    assert!(Running::LockedButUp.work_continues());

    let sleeping = Power { lid_action: LidAction::Sleep, external_display: false, ..on_a_stand() };
    assert_eq!(running_state(&sleeping, true, true), Running::Asleep);
    assert!(!Running::Asleep.work_continues());
}

#[test]
fn unlocked_and_in_front_of_you_is_plain_awake() {
    let open = Power { lid_closed: false, ..on_a_stand() };
    assert_eq!(running_state(&open, false, false), Running::Awake);
}

// ================= it checks rather than asking =================

fn all_clear() -> Checked {
    Checked {
        last_step_completed: true,
        world_unchanged: true,
        subject_still_there: true,
        already_sent_something: false,
        took_ms: 200,
    }
}

#[test]
fn it_works_out_whether_to_carry_on_rather_than_asking_you() {
    // You sat down to get on with something, not to answer questions about
    // last night.
    assert_eq!(on_waking_checked(&all_clear(), true, 600), OnWaking::Resume);
}

#[test]
fn how_long_it_slept_stops_mattering_once_the_world_has_been_checked() {
    // That rule existed only because nothing was being checked.
    assert_eq!(on_waking_checked(&all_clear(), true, 12 * 60), OnWaking::Resume);
}

#[test]
fn something_that_already_sent_is_the_one_thing_worth_stopping_for() {
    // No amount of checking makes a second email un-sent.
    let sent = Checked { already_sent_something: true, last_step_completed: false, ..all_clear() };
    assert_eq!(on_waking_checked(&sent, true, 10), OnWaking::AskFirst);
    let said = woke_checked("the update", OnWaking::AskFirst, &sent);
    assert!(said.contains("might send it twice"));
}

#[test]
fn something_that_changed_underneath_starts_again_when_that_is_safe() {
    let moved = Checked { world_unchanged: false, ..all_clear() };
    assert_eq!(on_waking_checked(&moved, true, 10), OnWaking::StartOver);
    assert_eq!(on_waking_checked(&moved, false, 10), OnWaking::AskFirst);
}

#[test]
fn something_whose_subject_has_gone_is_left_alone() {
    // Starting again would recreate it, which may be exactly what you didn't
    // want.
    let gone = Checked { subject_still_there: false, ..all_clear() };
    assert_eq!(on_waking_checked(&gone, true, 10), OnWaking::AskFirst);
    assert!(woke_checked("the export", OnWaking::AskFirst, &gone).contains("has gone"));
}

#[test]
fn a_check_that_took_too_long_is_not_trusted() {
    // You may want your brief the second you sit down.
    assert!(BUDGET_MS <= 2000);
    let slow = Checked { took_ms: BUDGET_MS + 1, ..all_clear() };
    assert_eq!(on_waking_checked(&slow, true, 10), OnWaking::AskFirst);
}

#[test]
fn what_it_says_leads_with_the_decision_not_the_reasoning() {
    let said = woke_checked("the index", OnWaking::Resume, &all_clear());
    assert!(said.starts_with("Picking the index back up"));
    assert!(said.len() < 90, "one line: {said}");
}
