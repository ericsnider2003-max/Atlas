//! The evening wrap-up and the Friday review (why-stale idea 2, 1 Oct 2026):
//! done, slipping, tomorrow's first move; the week in numbers.

use atlas::daily::{wrap_asked, wrap_due, wrap_said, Week, Wrap, WrapAsked, WRAP_AFTER_ACTIVE_SECS};

fn day() -> Wrap {
    Wrap {
        finished: vec!["send the Etsy listing".into(), "fix the oven quote".into(), "call the bank".into()],
        carried: vec![("edit the bakery video".into(), 4), ("reply to Sam".into(), 1)],
        handled: 2,
        active_secs: 6 * 3600 + 20 * 60,
        top: vec![("code".into(), 3 * 3600), ("video".into(), 2 * 3600)],
        longest_focus: Some(("code".into(), 95 * 60)),
        push: Some("your habits".into()),
        week: None,
    }
}

#[test]
fn the_day_is_done_slipping_and_tomorrows_first_move() {
    let said = wrap_said(&day());
    assert_eq!(
        said,
        "Today: 6 h 20 min at the machine -- code 3 h, video 2 h. Best stretch: 1 h 35 min on code. \
         Done: 3 things, including send the Etsy listing and fix the oven quote. I ran 2 jobs for you. \
         Slipping: edit the bakery video, carried 4 days, and 1 more still open. Tomorrow, start with edit the bakery video."
    );
}

#[test]
fn a_clear_list_hands_tomorrow_to_your_push_and_friday_adds_the_week() {
    let mut w = day();
    w.carried.clear();
    w.week = Some(Week { active_secs: 30 * 3600, days_worked: 5, focus_blocks: 9, finished: 14 });
    let said = wrap_said(&w);
    assert!(said.contains("Tomorrow, push on your habits."), "{said}");
    assert!(said.ends_with("This week: 30 h at the machine over 5 days, 9 long stretches of focus, 14 things finished."), "{said}");
    assert_eq!(wrap_said(&Wrap::default()), "Nothing to wrap up -- I've no record of today's work.");
}

#[test]
fn it_is_offered_once_an_evening_to_someone_who_worked_and_is_here() {
    let today = 20_000 * 86_400;
    assert!(wrap_due(19, today, 0, WRAP_AFTER_ACTIVE_SECS, true));
    assert!(!wrap_due(19, today, today, 5 * 3600, true), "already said today");
    assert!(!wrap_due(19, today, 0, 5 * 3600, false), "nobody at the desk");
    assert!(!wrap_due(15, today, 0, 5 * 3600, true), "still the afternoon");
    assert!(!wrap_due(19, today, 0, 20 * 60, true), "not a real day at the machine");
}

#[test]
fn it_is_asked_for_in_plain_words() {
    assert_eq!(wrap_asked("Wrap up my day."), Some(WrapAsked::Day));
    assert_eq!(wrap_asked("atlas, how did my week go?"), Some(WrapAsked::Week));
    assert_eq!(wrap_asked("wrap up the meeting notes"), None);
}

#[test]
fn asked_by_voice_it_reads_your_list() {
    use atlas::daemon::Daemon;
    use atlas::platform::{mock::MockPlatform, Monitor};
    use atlas::proactive::{Proactive, ProactiveConfig};
    use atlas::workspace_view::{Handoff, Item, Kind, Origin, Status};
    let c = atlas::config::Config::load(std::path::Path::new("config")).unwrap();
    let plat = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let dir = std::env::temp_dir().join(format!("atlas-end-of-day-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut d = Daemon::new(&c, &plat, None, atlas::store::Store::new(dir), Proactive::new(ProactiveConfig::default()));
    let t = atlas::store::now();
    let item = |title: &str, closed: Option<u64>| Item {
        id: title.into(),
        title: title.into(),
        kind: Kind::Task,
        status: if closed.is_some() { Status::Done } else { Status::Doing },
        due: None,
        project: None,
        client: None,
        links: Vec::new(),
        blocked_by: None,
        from: Origin::YouSaid,
        at: 0,
        closed_at: closed,
        tags: Vec::new(),
        estimate_mins: None,
        spent_mins: 0,
        atlas_could: Handoff::Unknown,
        thinking: Vec::new(),
    };
    d.workspace = vec![item("post the T-shirt designs", Some(t - 5)), item("edit the bakery video", None)];
    d.carried = vec![("edit the bakery video".into(), 3)];
    let said = d.turn("wrap up my day", t);
    assert!(said.contains("Done: post the T-shirt designs."), "{said}");
    assert!(said.contains("Slipping: edit the bakery video, carried 3 days."), "{said}");
    assert!(said.contains("Tomorrow, start with edit the bakery video."), "{said}");
}

// ---------- one thing I noticed (idea 11) ----------

use atlas::daily::{one_thing_noticed, DayLog};

fn worked(cat: &str, secs: u64, focus: usize) -> DayLog {
    DayLog { active_secs: secs, by_category: vec![(cat.into(), secs)], focus_blocks: focus, small_hours_secs: 0 }
}

#[test]
fn nothing_is_said_without_a_week_to_compare_against() {
    assert_eq!(one_thing_noticed(&vec![worked("code", 4 * 3600, 1); 6]), None);
    assert_eq!(one_thing_noticed(&vec![worked("code", 4 * 3600, 1); 8]), None, "an ordinary week says nothing");
}

#[test]
fn late_nights_come_first_and_name_the_count() {
    let mut days = vec![worked("code", 4 * 3600, 1); 8];
    for d in days.iter_mut().skip(4) {
        d.small_hours_secs = 40 * 60;
    }
    assert_eq!(one_thing_noticed(&days).as_deref(), Some("You've been at the machine after midnight on 4 of the last seven nights."));
}

#[test]
fn a_day_without_focus_or_far_more_of_one_thing_is_named_with_its_numbers() {
    let mut days = vec![worked("code", 4 * 3600, 1); 7];
    days.push(worked("code", 4 * 3600, 0));
    assert_eq!(
        one_thing_noticed(&days).as_deref(),
        Some("No stretch of 25 minutes on one thing today -- you had one on 6 of your last 6 working days.")
    );
    let mut days = vec![worked("video", 3600, 1); 7];
    days.push(worked("video", 3 * 3600, 1));
    assert_eq!(one_thing_noticed(&days).as_deref(), Some("3 h on video today -- about twice your usual 1 h."));
}

#[test]
fn the_morning_brief_carries_it_after_the_work() {
    let mut b = atlas::brief::run(&[], &[], &atlas::brief::BriefConfig::default());
    b.noticed = Some("You've been at the machine after midnight on 4 of the last seven nights.".into());
    assert!(!b.is_empty());
    assert_eq!(atlas::brief::spoken(&b), "One thing I noticed: You've been at the machine after midnight on 4 of the last seven nights.");
}
