use atlas::brief::{
    run, spoken, Brief, BriefConfig, Commitment, Item, Outcome, Weight, MAX_LINES, NEVER_SENDS,
};
use atlas::council::{Council, Disposition, Lean, Opinion, Round, Seat, MAX_SEATS, MIN_SEATS};

// ========================= the council =========================

fn op(seat: &str, lean: Lean, because: &str) -> Opinion {
    Opinion {
        seat: seat.into(),
        lean,
        because: because.into(),
        would_change_my_mind: Some("evidence".into()),
    }
}

#[test]
fn seats_are_prompted_blind_so_nobody_sees_anybody() {
    let c = Council::default_room();
    let prompts = c.blind_prompts("should we ship on Friday?");
    assert_eq!(prompts.len(), c.seats.len());
    // No prompt may contain another seat's brief, or the round is not blind.
    for (id, p) in &prompts {
        for other in c.seats.iter().filter(|s| &s.id != id) {
            assert!(
                !p.contains(&other.brief),
                "{id}'s prompt leaks {}'s brief — the round is not blind",
                other.id
            );
        }
    }
}

#[test]
fn a_room_that_all_thinks_alike_is_one_seat_five_times() {
    let same = Council::new(vec![
        Seat::new("a", "one", Disposition::Sceptic),
        Seat::new("b", "two", Disposition::Sceptic),
        Seat::new("c", "three", Disposition::Sceptic),
    ]);
    assert!(!same.covers_different_ground());
    assert!(Council::default_room().covers_different_ground());
}

#[test]
fn quorum_has_a_floor_and_a_ceiling() {
    let one = Council::new(vec![Seat::new("a", "one", Disposition::Bias)]);
    assert!(!one.is_quorate(), "two people is a conversation, not a council");
    assert!(Council::default_room().is_quorate());
    let mob: Vec<Seat> = (0..MAX_SEATS + 1)
        .map(|i| Seat::new(&format!("s{i}"), "x", Disposition::Bias))
        .collect();
    assert!(!Council::new(mob).is_quorate(), "past {MAX_SEATS} nobody reads it");
    assert!(MIN_SEATS >= 3);
}

#[test]
fn the_verdict_always_names_the_split_even_when_the_room_agreed() {
    let c = Council::default_room();
    let v = c.tally(
        &[op("mover", Lean::For, "cheap"), op("sceptic", Lean::For, "fine")],
        Round::Open,
    );
    assert!(v.split.contains("2 for"), "{}", v.split);
}

#[test]
fn the_losing_argument_survives_into_the_verdict() {
    let c = Council::default_room();
    let v = c.tally(
        &[
            op("mover", Lean::For, "short"),
            op("operator", Lean::For, "also short"),
            op("steward", Lean::Against, "this commits us to maintaining it for years"),
        ],
        Round::Open,
    );
    assert_eq!(v.call, Some(Lean::For));
    let d = v.strongest_dissent.expect("burying the dissent makes this a rubber stamp");
    assert!(d.contains("steward"));
    assert!(d.contains("maintaining it for years"));
}

#[test]
fn unanimity_on_the_blind_round_is_reported_as_a_warning() {
    let c = Council::default_room();
    let all_for = vec![
        op("mover", Lean::For, "a"),
        op("sceptic", Lean::For, "b"),
        op("operator", Lean::For, "c"),
    ];
    let blind = c.tally(&all_for, Round::Blind);
    assert!(blind.suspiciously_unanimous);
    assert!(c.spoken(&blind).contains("told them the answer"));

    // After they have seen each other, agreement is just agreement.
    let open = c.tally(&all_for, Round::Open);
    assert!(!open.suspiciously_unanimous);
}

#[test]
fn a_tie_is_broken_by_the_tiebreak_seat_and_not_by_averaging() {
    let c = Council::default_room();
    let v = c.tally(
        &[
            op("mover", Lean::For, "ship it"),
            op("sceptic", Lean::Against, "prove it"),
            op("steward", Lean::Against, "we keep this forever"),
        ],
        Round::Open,
    );
    assert_eq!(v.call, Some(Lean::Against));
}

#[test]
fn a_room_with_no_tiebreaker_is_allowed_to_have_no_answer() {
    let c = Council::new(vec![
        Seat::new("a", "one", Disposition::Bias),
        Seat::new("b", "two", Disposition::Sceptic),
        Seat::new("c", "three", Disposition::Operator),
    ]);
    let v = c.tally(
        &[op("a", Lean::For, "x"), op("b", Lean::Against, "y"), op("c", Lean::Depends, "z")],
        Round::Open,
    );
    assert_eq!(v.call, None);
    assert!(c.spoken(&v).contains("couldn't come to an agreement"));
    assert!(c.spoken(&v).contains("asking for guidance"));
}

// ========================= the morning brief =========================

fn cfg_on() -> BriefConfig {
    let mut c = BriefConfig::default();
    c.enabled = true;
    c
}

fn item(id: &str, from: &str, w: Weight, o: Outcome) -> Item {
    Item {
        id: id.into(),
        // These tests are about the mail-shaped path specifically, which is
        // still the shape `from_mail` produces. The offline sources have
        // their own file.
        source: atlas::brief::Source::Mail,
        from: from.into(),
        subject: "something".into(),
        weight: w,
        outcome: o,
        draft: None,
        conflicts_with: None,
    }
}

#[test]
fn nothing_runs_when_it_is_switched_off() {
    // `BriefConfig::default()` used to be this case. It ships on now that the
    // brief reads this machine rather than an inbox it has no reader for, so
    // the off case has to be built deliberately — and it still has to hold,
    // because "off" must mean nothing is computed rather than nothing is said.
    let mut off = BriefConfig::default();
    off.enabled = false;
    let b = run(&[item("1", "a", Weight::Urgent, Outcome::Yours)], &[], &off);
    assert!(b.is_empty());
}

#[test]
fn the_brief_leads_with_what_to_do_not_with_a_count() {
    let items = vec![
        item("1", "Priya", Weight::Urgent, Outcome::Yours),
        item("2", "noise", Weight::Info, Outcome::Handled),
        item("3", "more noise", Weight::Info, Outcome::Handled),
    ];
    let b = run(&items, &[], &cfg_on());
    assert_eq!(b.start_with.as_deref(), Some("Priya — something"));
    let s = spoken(&b);
    assert!(s.starts_with("Start with Priya"), "{s}");
    // The count is present but not first.
    assert!(s.contains("I handled 2"));
}

#[test]
fn a_draft_is_never_sent_and_the_separation_is_named() {
    assert!(NEVER_SENDS.contains("approval"));
    let mut c = cfg_on();
    c.draft_replies = true;
    let mut it = item("1", "Sam", Weight::Urgent, Outcome::Drafted);
    it.draft = Some("Hi Sam,".into());
    let b = run(&[it], &[], &c);
    assert_eq!(b.drafted.len(), 1);
    for d in &b.drafted {
        assert_eq!(d.outcome, Outcome::Drafted, "nothing in a brief is ever Sent");
    }
    assert!(spoken(&b).contains("waiting on your yes"));
}

#[test]
fn a_claimed_draft_with_nothing_written_becomes_yours_instead() {
    let mut c = cfg_on();
    c.draft_replies = true;
    // Outcome says Drafted, but there is no draft. That is a claim.
    let b = run(&[item("1", "Sam", Weight::Urgent, Outcome::Drafted)], &[], &c);
    assert!(b.drafted.is_empty());
    assert_eq!(b.yours.len(), 1);
    assert_eq!(b.yours[0].outcome, Outcome::Yours);
}

#[test]
fn drafting_off_means_drafts_come_to_you_rather_than_vanishing() {
    let mut it = item("1", "Sam", Weight::Urgent, Outcome::Drafted);
    it.draft = Some("written anyway".into());
    let b = run(&[it], &[], &cfg_on()); // draft_replies defaults off
    assert!(b.drafted.is_empty());
    assert_eq!(b.yours.len(), 1);
    assert!(b.yours[0].draft.is_none(), "an unshown draft should not be carried along");
}

#[test]
fn things_below_the_floor_are_counted_not_lost() {
    let items = vec![
        item("1", "a", Weight::Ignore, Outcome::Yours),
        item("2", "b", Weight::Ignore, Outcome::Yours),
        item("3", "c", Weight::Urgent, Outcome::Yours),
    ];
    let b = run(&items, &[], &cfg_on());
    assert_eq!(b.ignored, 2);
    assert_eq!(b.yours.len(), 1);
}

#[test]
fn urgent_comes_before_merely_informative() {
    let items = vec![
        item("1", "info", Weight::Info, Outcome::Yours),
        item("2", "urgent", Weight::Urgent, Outcome::Yours),
    ];
    let b = run(&items, &[], &cfg_on());
    assert_eq!(b.yours[0].from, "urgent");
}

#[test]
fn overlapping_commitments_are_found() {
    let day = vec![
        Commitment { id: "a".into(), what: "standup".into(), at_minute: 540, minutes: 30, needs_prep: None },
        Commitment { id: "b".into(), what: "review".into(), at_minute: 555, minutes: 60, needs_prep: None },
    ];
    let b = run(&[], &day, &cfg_on());
    assert!(b.conflicts.iter().any(|c| c.contains("standup overlaps review")));
}

#[test]
fn back_to_back_is_not_an_overlap() {
    let day = vec![
        Commitment { id: "a".into(), what: "one".into(), at_minute: 540, minutes: 30, needs_prep: None },
        Commitment { id: "b".into(), what: "two".into(), at_minute: 570, minutes: 30, needs_prep: None },
    ];
    let b = run(&[], &day, &cfg_on());
    assert!(b.conflicts.is_empty(), "{:?}", b.conflicts);
}

#[test]
fn prep_that_has_not_happened_is_raised_before_the_meeting_is() {
    let day = vec![Commitment {
        id: "a".into(),
        what: "the board call".into(),
        at_minute: 600,
        minutes: 60,
        needs_prep: Some("the numbers".into()),
    }];
    let b = run(&[], &day, &cfg_on());
    assert!(b.conflicts.iter().any(|c| c.contains("the board call needs the numbers first")));
}

#[test]
fn the_one_thing_holding_up_several_others_is_named() {
    let mut a = item("1", "x", Weight::Info, Outcome::Yours);
    a.conflicts_with = Some("the contract".into());
    let mut b2 = item("2", "y", Weight::Info, Outcome::Yours);
    b2.conflicts_with = Some("the contract".into());
    let mut c = item("3", "z", Weight::Info, Outcome::Yours);
    c.conflicts_with = Some("something else".into());
    let b = run(&[a, b2, c], &[], &cfg_on());
    let bl = b.blocking.expect("two things waiting on one thing is a bottleneck");
    assert!(bl.contains("the contract"));
    assert!(bl.contains("2 other things"));
}

#[test]
fn a_bottleneck_is_not_invented_when_there_is_not_one() {
    let mut a = item("1", "x", Weight::Info, Outcome::Yours);
    a.conflicts_with = Some("the contract".into());
    let b = run(&[a], &[], &cfg_on());
    assert!(b.blocking.is_none(), "one thing waiting on one thing is not a bottleneck");
}

#[test]
fn an_empty_morning_says_so_plainly() {
    let b = run(&[item("1", "a", Weight::Info, Outcome::Handled)], &[], &cfg_on());
    assert!(b.is_empty());
    assert_eq!(spoken(&b), "Nothing needs you. I'll get on with the rest.");
}

#[test]
fn the_brief_stays_short_on_the_worst_morning_of_the_year() {
    let items: Vec<Item> = (0..200)
        .map(|i| item(&format!("{i:03}"), "someone", Weight::Urgent, Outcome::Yours))
        .collect();
    let b = run(&items, &[], &cfg_on());
    assert_eq!(b.yours.len(), 200, "nothing is dropped from the data");
    let lines = spoken(&b).split(". ").count();
    assert!(lines <= MAX_LINES, "the brief grew to {lines} lines and stopped being read");
}

#[test]
fn ordering_is_stable_so_two_runs_of_the_same_morning_match() {
    let items = vec![
        item("b", "one", Weight::Urgent, Outcome::Yours),
        item("a", "two", Weight::Urgent, Outcome::Yours),
    ];
    let first = run(&items, &[], &cfg_on());
    let second = run(&items, &[], &cfg_on());
    assert_eq!(first, second);
    assert_eq!(first.yours[0].id, "a");
}

// ========================= they meet at the nudge =========================

#[test]
fn the_morning_nudge_carries_the_brief_rather_than_just_a_greeting() {
    use atlas::nudge::{daypart_with_brief, Part, Trigger};
    let items = vec![item("1", "Priya", Weight::Urgent, Outcome::Yours)];
    let n = daypart_with_brief(Part::Morning, &atlas::brief::run(&items, &[], &cfg_on()));
    assert_eq!(n.trigger, Trigger::Daypart);
    assert!(n.message.starts_with("Good morning"));
    assert!(n.message.contains("Start with Priya"));
}

#[test]
fn convening_produces_one_prompt_per_seat_and_a_quorate_room() {
    let (c, prompts) = atlas::nudge::convene("do we keep paying for this?");
    assert!(c.is_quorate());
    assert!(c.covers_different_ground());
    assert_eq!(prompts.len(), c.seats.len());
}

#[test]
fn a_brief_with_only_drafts_still_tells_you_where_to_start() {
    let mut c = cfg_on();
    c.draft_replies = true;
    let mut it = item("1", "Sam", Weight::Info, Outcome::Drafted);
    it.draft = Some("hi".into());
    let b: Brief = run(&[it], &[], &c);
    assert_eq!(b.start_with.as_deref(), Some("approve the reply to Sam"));
}

/// 30 Sep 2026: the spoken brief named the first thing needing you and none
/// of the rest; the next few are named, and the remainder counted.
#[test]
fn the_spoken_brief_names_more_than_the_first_thing() {
    let items: Vec<Item> = ["Priya", "Sam", "Jo", "Lee", "Max", "Ana"]
        .iter()
        .enumerate()
        .map(|(i, who)| item(&i.to_string(), who, Weight::Urgent, Outcome::Yours))
        .collect();
    let b = run(&items, &[], &cfg_on());
    let s = spoken(&b);
    let first = b.start_with.clone().unwrap();
    assert!(s.starts_with(&format!("Start with {first}.")), "{s}");
    let also = s.split("Also: ").nth(1).expect("the rest named").split('.').next().unwrap();
    assert_eq!(also.split("; ").count(), atlas::brief::ALSO_NAMED, "{also}");
    assert!(also.ends_with(", and 2 more on the hub"), "{also}");
    assert!(!also.contains(first.as_str()), "the first isn't named twice: {s}");
}
