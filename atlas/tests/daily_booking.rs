use atlas::booking::{
    answered, assess, could_offer, going_stale, stale_nudge, to_decide, BookingConfig, Fit,
    Proposal, Slot, State,
};
use atlas::daily::{close, day_of, has_rolled, opening, Carried, DailyConfig};
use atlas::messaging::{filed, folder_for, note_on, Folder, Message, Platform};
use atlas::workspace_view::{
    could_hand_over, spoken, Handoff, Item, Kind, Origin, Status,
};

const DAY: u64 = 86_400;
const HOUR: u64 = 3600;

fn item(title: &str, status: Status, at: u64, closed: Option<u64>, est: Option<u32>, spent: u32, handoff: Handoff) -> Item {
    Item {
        id: title.into(), title: title.into(), kind: Kind::Task, status,
        due: None, project: None, client: None, links: vec![], blocked_by: None,
        from: Origin::YouSaid, at, closed_at: closed, tags: vec![], thinking: vec![],
        estimate_mins: est, spent_mins: spent, atlas_could: handoff,
    }
}

// ================= the day as a unit =================

#[test]
fn working_until_two_in_the_morning_is_still_yesterdays_list() {
    // Pinned here (28 Sep 2026): this is about a time of day, and it passed
    // only when cargo found `.cargo/config.toml` (run from the crate's folder).
    atlas::localclock::pin_offset(Some(0));
    // Telling someone otherwise is pedantry.
    let cfg = DailyConfig { rolls_at_hour: 4, ..Default::default() };
    let two_am = 10 * DAY + 2 * HOUR;
    assert_eq!(day_of(two_am, &cfg), 9 * DAY);
    let ten_am = 10 * DAY + 10 * HOUR;
    assert_eq!(day_of(ten_am, &cfg), 10 * DAY);
}

#[test]
fn the_day_turning_is_noticed() {
    // Pinned here (28 Sep 2026): this is about a time of day, and it passed
    // only when cargo found `.cargo/config.toml` (run from the crate's folder).
    atlas::localclock::pin_offset(Some(0));
    let cfg = DailyConfig::default();
    assert!(!has_rolled(10 * DAY + HOUR, 10 * DAY + 5 * HOUR, &cfg));
    assert!(has_rolled(10 * DAY + 23 * HOUR, 11 * DAY + HOUR, &cfg));
}

#[test]
fn unfinished_work_carries_and_the_count_goes_up() {
    let items = vec![
        item("finish the report", Status::Doing, 5 * DAY, None, None, 0, Handoff::NeedsYou),
        item("file the thing", Status::Done, 5 * DAY, Some(10 * DAY + HOUR), None, 0, Handoff::NeedsYou),
    ];
    let before = vec![("finish the report".to_string(), 3u32)];
    let (closed, carried) = close(&items, 10 * DAY, &before, &DailyConfig::default());

    assert_eq!(closed.finished, vec!["file the thing"]);
    assert_eq!(carried[0].1, Carried(4), "fourth day, not first");
}

#[test]
fn a_task_on_its_eighth_day_is_one_you_have_decided_not_to_do_eight_times() {
    // The number that matters and no task app shows.
    let after = DailyConfig::default().ask_after_days;
    assert_eq!(after, 5, "the shipped threshold, which these cases are written around");
    assert!(!Carried(3).worth_mentioning(after));
    assert!(Carried(6).worth_mentioning(after));
    assert!(Carried(6).nudge("x", after).unwrap().contains("Still worth doing"));
    assert!(Carried(12).nudge("x", after).unwrap().contains("I'd drop it"));
    assert!(Carried(12).nudge("x", after).unwrap().contains("costing you a line every morning"));

    // And the threshold is yours. `daily.ask_after_days` shipped 5 and was
    // read by nothing: 5 was written into `worth_mentioning` and again as
    // `0..=4` in `nudge`, so the file and the behaviour agreed by accident.
    assert!(Carried(3).worth_mentioning(3), "asked to be asked on day three");
    assert!(Carried(3).nudge("x", 3).unwrap().contains("Still worth doing"));
    assert!(Carried(6).nudge("x", 3).unwrap().contains("I'd drop it"), "twice the threshold");
    assert!(!Carried(9).worth_mentioning(10), "and it can be pushed the other way");
}

#[test]
fn the_working_out_never_resets_with_the_day() {
    // A task on its eighth day without its history is just a task you forgot.
    //
    // This asserted `thinking_carries`, a `#[serde(skip)]` bool pinned true
    // that nothing read. Deleted 19 Sep 2026: what makes the promise true is
    // that `close` takes `&[Item]` -- an immutable borrow, so the day rolling
    // over structurally cannot clear anything's thinking -- and `thread`
    // hands that thinking back with the carry count.
    let src = std::fs::read_to_string("src/daily.rs").expect("src/daily.rs");
    let sig = src.split("pub fn close(").nth(1).expect("close is gone");
    let sig = &sig[..sig.find(')').expect("unterminated signature")];
    assert!(
        sig.contains("items: &[crate::workspace_view::Item]"),
        "`close` takes the day's items by shared reference, which is why rolling the day \
         cannot erase what you worked out: {sig}"
    );
    assert!(
        src.contains("&item.thinking"),
        "`thread` no longer hands the thinking back with the carry"
    );

    // And an old config naming the removed key still loads.
    let parsed: DailyConfig =
        serde_yaml::from_str("enabled: true\nthinking_carries: false\n").unwrap();
    assert!(parsed.enabled, "an unknown key must not stop the section parsing");
}

#[test]
fn what_you_hear_when_the_day_turns_is_short() {
    // It arrives when you sit down, and a paragraph about yesterday is the
    // wrong thing to read first.
    let items = vec![item("a thing", Status::Doing, 0, None, None, 0, Handoff::NeedsYou)];
    let (closed, carried) = close(&items, 10 * DAY, &[("a thing".into(), 7)], &DailyConfig::default());
    // Behaviour, not just wording: the count in the opening comes from a real
    // carried item -- the live task was carried, not finished.
    assert_eq!(carried.len(), 1, "the carried count does not match a real carried item");
    assert!(closed.finished.is_empty(), "an unfinished task was reported as done");
    let said = opening(&closed, &carried, &DailyConfig::default());
    assert!(said.contains("1 carried over"));
    assert!(said.contains("Still worth doing"), "the one that's been moving is mentioned");
}

#[test]
fn a_clean_list_says_so_in_three_words() {
    let (closed, carried) = close(&[], 10 * DAY, &[], &DailyConfig::default());
    assert_eq!(opening(&closed, &carried, &DailyConfig::default()), "Clean list today.");
}

// ================= what Atlas could take off you =================

#[test]
fn the_mark_that_saves_the_most_time_is_this_did_not_need_to_be_your_job() {
    let items = vec![
        item("pull the fee data", Status::Waiting, 0, None, Some(40), 0, Handoff::Yes),
        item("decide on the VPS", Status::NeedsYou, 0, None, Some(10), 0, Handoff::NeedsYou),
        item("draft the reply", Status::Waiting, 0, None, Some(15), 0, Handoff::Yes),
    ];
    let mine = could_hand_over(&items);
    assert_eq!(mine.len(), 2);
    assert_eq!(mine[0].title, "pull the fee data", "the longest one first");
    assert!(spoken(&items, 0).contains("2 of these I can just do"));
    assert!(spoken(&items, 0).contains("55 minutes of your time"));
}

#[test]
fn something_atlas_can_only_half_do_is_not_started_alone() {
    // A half-done thing and no idea where it stopped is worse than not
    // starting.
    assert!(Handoff::Yes.safe_to_start_alone());
    assert!(!Handoff::MostOfIt.safe_to_start_alone());
    assert!(!Handoff::PrepOnly.safe_to_start_alone());
    assert_eq!(Handoff::MostOfIt.mark(), "I can get most of the way");
}

// ================= times with other people =================

fn proposal(times: Vec<Slot>) -> Proposal {
    Proposal {
        id: 1,
        from: "Marta".into(),
        about: Some("the certification".into()),
        their_words: "Could we do Tuesday morning?".into(),
        times,
        at: 0,
        state: State::NeedsYou,
    }
}

#[test]
fn atlas_never_books_or_replies_on_its_own() {
    // A time with someone else is a promise made in your name.
    let cfg = BookingConfig::default();
    assert!(!cfg.may_book_alone);
    assert!(!cfg.may_send_alone);
    let parsed: BookingConfig =
        serde_yaml::from_str("enabled: true\nmay_book_alone: true\nmay_send_alone: true\n").unwrap();
    assert!(!parsed.may_book_alone);
    assert!(!parsed.may_send_alone);
}

#[test]
fn each_offered_time_is_judged_against_what_you_already_have() {
    let cfg = BookingConfig { enabled: true, ..Default::default() };
    let now = 10 * DAY;
    let busy = vec![Slot { start: 12 * DAY + 10 * HOUR, mins: 60 }];
    let p = proposal(vec![
        Slot { start: 12 * DAY + 10 * HOUR, mins: 30 },
        Slot { start: 12 * DAY + 14 * HOUR, mins: 30 },
        Slot { start: 12 * DAY + 23 * HOUR, mins: 30 },
        Slot { start: 5 * DAY, mins: 30 },
    ]);
    let a = assess(&p, &busy, now, &cfg, &atlas::tz::Zone::utc());
    assert_eq!(a[0].verdict, Fit::Clashes);
    assert_eq!(a[1].verdict, Fit::Free);
    assert_eq!(a[2].verdict, Fit::OffHours);
    assert_eq!(a[3].verdict, Fit::Past);
}

#[test]
fn a_counter_offer_is_never_eleven_at_night() {
    // Worse than saying no.
    let cfg = BookingConfig { enabled: true, hours: (9, 18), ..Default::default() };
    let offers = could_offer(&[], 10 * DAY, 30, &cfg, 3, &atlas::tz::Zone::utc());
    assert_eq!(offers.len(), 3);
    for o in offers {
        let hour = (o.start % 86_400) / 3600;
        assert!((9..18).contains(&hour), "offered {hour}:00");
    }
}

#[test]
fn atlas_stops_and_says_nothing_is_booked() {
    let cfg = BookingConfig { enabled: true, ..Default::default() };
    let p = proposal(vec![Slot { start: 12 * DAY + 14 * HOUR, mins: 30 }]);
    let a = assess(&p, &[], 10 * DAY, &cfg, &atlas::tz::Zone::utc());
    let said = to_decide(&p, &a, &[]);
    assert!(said.contains("Marta wants a time about the certification"));
    assert!(said.contains("One of their times is clear"));
    assert!(said.ends_with("Nothing's booked — say yes, no, or a different time."));
}

#[test]
fn when_none_of_their_times_work_it_offers_alternatives_rather_than_just_refusing() {
    let cfg = BookingConfig { enabled: true, ..Default::default() };
    let p = proposal(vec![Slot { start: 5 * DAY, mins: 30 }]);
    let a = assess(&p, &[], 10 * DAY, &cfg, &atlas::tz::Zone::utc());
    let alts = could_offer(&[], 10 * DAY, 30, &cfg, 3, &atlas::tz::Zone::utc());
    let said = to_decide(&p, &a, &alts);
    assert!(said.contains("None of their times work"));
    assert!(said.contains("3 you could offer"));
}

#[test]
fn your_answer_is_understood_including_a_different_time() {
    assert_eq!(answered("yes"), Some(State::Accepted));
    assert_eq!(answered("no, can't do that week"), Some(State::Declined));
    assert_eq!(answered("how about Thursday instead"), Some(State::CounterOffered));
    assert!(answered("hmm").is_none());
}

#[test]
fn something_waiting_while_its_times_pass_is_raised() {
    // It waits politely on the hub until the meeting it was about is in the
    // past.
    let now = 10 * DAY;
    let soon = proposal(vec![Slot { start: 10 * DAY + 20 * HOUR, mins: 30 }]);
    let waiting = vec![soon];
    let stale = going_stale(&waiting, now);
    assert_eq!(stale.len(), 1);
    let said = stale_nudge(stale[0]);
    assert!(said.contains("a no by default, which is a worse answer than a no"));
}

// ================= who goes in which folder =================

fn msg(from: &str, group: Option<&str>, text: &str, at: u64) -> Message {
    Message {
        id: "1".into(), platform: Platform::Telegram, from: from.into(),
        group: group.map(str::to_string), text: text.into(), at, mentions_you: false,
    }
}

#[test]
fn one_message_decides_nothing_because_a_single_hi_tells_you_nothing() {
    assert_eq!(folder_for(&[], &[]), Folder::Unsorted);
    assert_eq!(folder_for(&[msg("Sam", None, "hi", 0)], &[]), Folder::Unsorted);
}

#[test]
fn one_business_approach_is_a_prospect_and_a_conversation_of_them_is_work() {
    // Different folders, deliberately.
    let one = vec![msg("Nadia", None, "we'd love to work with you on a paid partnership", 0)];
    assert_eq!(folder_for(&one, &[]), Folder::Prospect);

    let many = vec![
        msg("Nadia", None, "we'd love to work with you on a paid partnership", 0),
        msg("Nadia", None, "here's the brief and the budget", 1),
        msg("Nadia", None, "sending the contract over", 2),
    ];
    assert_eq!(folder_for(&many, &[]), Folder::Work);
}

#[test]
fn friends_services_and_groups_all_land_somewhere_sensible() {
    assert_eq!(folder_for(&[msg("Dave", None, "pub on the weekend mate?", 0)], &[]), Folder::Personal);
    assert_eq!(folder_for(&[msg("Shop", None, "your order has shipped", 0)], &[]), Folder::NotAPerson);
    assert_eq!(folder_for(&[msg("Dave", Some("football"), "6pm", 0)], &[]), Folder::Group);
}

#[test]
fn the_note_quotes_them_rather_than_paraphrasing() {
    // A note that paraphrases wrongly is worse than none.
    let msgs = vec![
        msg("Nadia", None, "we'd love to work with you on a paid partnership for the fee series", 0),
        msg("Nadia", None, "budget is confirmed", 100),
    ];
    let p = note_on(&msgs, &[]).unwrap();
    assert_eq!(p.name, "Nadia");
    assert_eq!(p.messages, 2);
    assert!(p.first_about.contains("paid partnership"));
    assert_eq!(p.last_at, 100);
}

#[test]
fn atlas_only_speaks_up_when_filing_someone_is_actually_useful() {
    // "I've filed someone as unsorted" is not worth a sentence.
    let work = note_on(
        &[
            msg("Nadia", None, "paid partnership brief", 0),
            msg("Nadia", None, "contract attached", 1),
            msg("Nadia", None, "budget confirmed", 2),
            msg("Nadia", None, "usage rights ok", 3),
            msg("Nadia", None, "invoice", 4),
        ],
        &[],
    )
    .unwrap();
    // Eric, 25 Sep 2026 (F3): what they want, and an offer to answer.
    let said = filed(&work).unwrap();
    assert!(said.starts_with("It looks like Nadia wants to know about"), "{said}");
    assert!(said.ends_with("Would you like me to look for the answer and respond?"), "{said}");

    let quiet = note_on(&[msg("Sam", None, "hi", 0)], &[]).unwrap();
    assert!(filed(&quiet).is_none());
}
