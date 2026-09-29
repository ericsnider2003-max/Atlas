use atlas::capture::{found, handles, kind_of, CaptureConfig, Kind, Notebook};
use atlas::content::{
    before_posting, edits_it_can_do, faults, hook_of, how_its_going, learn, ContentConfig, Fault,
    Hook, Performance, Piece,
};
use atlas::returning::{address_change, confirm_address, Address};

const DAY: u64 = 86_400;

// ================= telling Atlas what to call you =================

#[test]
fn you_can_change_what_atlas_calls_you_by_saying_so() {
    // One sentence, not a trip to settings.
    assert_eq!(address_change("call me Eric"), Some(Address::Name("Eric".into())));
    assert_eq!(address_change("address me as sir"), Some(Address::Title("sir".into())));
    assert_eq!(address_change("you can call me boss"), Some(Address::Title("boss".into())));
}

#[test]
fn a_nickname_is_a_name_not_a_title() {
    assert_eq!(address_change("call me Mint"), Some(Address::Name("Mint".into())));
}

#[test]
fn you_can_tell_it_to_stop() {
    assert_eq!(address_change("stop calling me that"), Some(Address::None));
    assert_eq!(address_change("don't call me anything"), Some(Address::None));
}

#[test]
fn an_ordinary_sentence_changes_nothing() {
    assert!(address_change("what's on my calendar").is_none());
    assert!(address_change("call me back later").is_none());
}

#[test]
fn it_uses_the_new_one_immediately_so_you_can_hear_it() {
    assert_eq!(confirm_address(&Address::Name("Eric".into())), "Got it, Eric.");
    assert_eq!(confirm_address(&Address::Title("sir".into())), "Very good, sir.");
    assert_eq!(confirm_address(&Address::None), "Right — no name.");
}

// ================= capture, then file =================

fn cfg() -> CaptureConfig {
    CaptureConfig { projects: vec!["Homelab".into()], ..Default::default() }
}

#[test]
fn capturing_asks_nothing_because_asking_is_what_stops_you() {
    // Deciding where it goes, at the moment you have the thought, is exactly
    // what loses the thought.
    assert!(CaptureConfig::default().never_ask_on_capture);
    let mut n = Notebook::default();
    let id = n.capture("broker fees are 40% higher on the new tier", None, 0, &cfg());
    assert_eq!(n.acknowledge(id), "Got it.");
}

#[test]
fn a_task_lands_on_the_list_and_says_so() {
    let mut n = Notebook::default();
    let id = n.capture("remind me to chase the certification", None, 0, &cfg());
    assert_eq!(n.notes[0].kind, Kind::Task);
    assert!(n.acknowledge(id).contains("on the list"));
}

#[test]
fn what_sort_of_thing_it_is_comes_from_how_you_said_it() {
    assert_eq!(kind_of("need to call the broker"), Kind::Task);
    assert_eq!(kind_of("what if we ran it on the second machine"), Kind::Idea);
    assert_eq!(kind_of("decided to stay on the VPS"), Kind::Decision);
    assert_eq!(kind_of("why does the certification take six weeks?"), Kind::Question);
    assert_eq!(kind_of("the fee is 40 basis points"), Kind::Fact);
}

#[test]
fn what_you_were_doing_is_kept_because_it_is_the_best_clue_and_free() {
    let h = handles("the fee is higher", Some("Chrome"), &[]);
    assert!(h.iter().any(|x| x.contains("while in Chrome")));
}

#[test]
fn handles_are_what_you_would_search_for_not_a_folder() {
    // You won't remember the folder. You'll remember a word, or when, or what
    // you were doing.
    let h = handles("Homelab fees went to 40 basis points on Tier 2", None, &["Homelab".into()]);
    assert!(h.contains(&"Homelab".to_string()));
    assert!(h.iter().any(|x| x.contains("Tier")), "proper nouns: {h:?}");
    assert!(h.iter().any(|x| x == "40"), "numbers: {h:?}");
}

#[test]
fn asking_where_something_is_finds_it_from_a_word_you_remember() {
    let mut n = Notebook::default();
    n.capture("broker fees went to 40 basis points on the new tier", None, 0, &cfg());
    n.capture("the oven runs twenty degrees hot", None, 0, &cfg());
    let hits = n.find("where's that thing about the broker fees", DAY);
    assert_eq!(hits.len(), 1);
    assert!(hits[0].text.contains("basis points"));
}

#[test]
fn roughly_when_it_was_also_finds_it() {
    // Two notes about the same thing; "last week" picks the right one.
    let mut n = Notebook::default();
    let now = 30 * DAY;
    n.capture("broker fees, the old note", None, now - 25 * DAY, &cfg());
    n.capture("broker fees, the recent one", None, now - 10 * DAY, &cfg());
    let hits = n.find("that broker fees note from last week", now);
    assert!(hits[0].text.contains("recent one"), "got: {}", hits[0].text);
}

#[test]
fn finding_it_says_what_you_were_doing_at_the_time() {
    let mut n = Notebook::default();
    n.capture("check the allowlist", Some("Chrome"), 0, &cfg());
    let said = found(&n.find("allowlist", DAY));
    assert!(said.contains("while you were in Chrome"));
}

#[test]
fn finding_nothing_says_so_rather_than_offering_the_nearest_thing() {
    let mut n = Notebook::default();
    n.capture("the oven runs hot", None, 0, &cfg());
    assert!(found(&n.find("submarines", DAY)).contains("can't find that one"));
}

#[test]
fn correcting_the_filing_is_worth_more_than_the_original_guess() {
    let mut n = Notebook::default();
    let id = n.capture("the fee is 40 points", None, 0, &cfg());
    assert!(n.correct(id, Some(Kind::Task), Some("Homelab")));
    assert_eq!(n.notes[0].kind, Kind::Task);
    assert!(n.notes[0].confirmed);
    assert_eq!(n.find("Homelab", DAY).len(), 1);
}

#[test]
fn ideas_that_went_nowhere_are_surfaced_once_not_nagged_about() {
    // The point of frictionless capture is that some of it is rubbish.
    let mut n = Notebook::default();
    n.capture("what if we cached the whole thing", None, 0, &cfg());
    n.capture("need to call the broker", None, 0, &cfg());
    let stale = n.never_revisited(90 * DAY, 60);
    assert_eq!(stale.len(), 1, "only the idea, not the task");
}

// ================= running your content =================

fn weak() -> Piece {
    Piece {
        first_line: "So today I wanted to talk about trading fees".into(),
        script: "So today I wanted to talk about trading fees and how they work generally \
                 across the industry and what you might want to think about".into(),
        seconds: 48.0,
        value_at_secs: 9.0,
        has_specifics: false,
        lands: false,
    }
}

fn strong() -> Piece {
    Piece {
        first_line: "Stop paying tier two fees".into(),
        script: "Stop paying tier two fees. Mine went from 40 basis points to 12 by asking one \
                 question. Here's the question, and here's what they said when I asked it. \
                 Ask yours the same thing this week.".into(),
        seconds: 24.0,
        value_at_secs: 1.5,
        has_specifics: true,
        lands: true,
    }
}

#[test]
fn the_opening_is_read_for_what_kind_it_is() {
    assert_eq!(hook_of("Stop paying tier two fees"), Hook::Contradiction);
    assert_eq!(hook_of("If you trade futures, this matters"), Hook::Called);
    assert_eq!(hook_of("This cost me four thousand dollars to learn"), Hook::Unfinished);
    assert_eq!(hook_of("So today I wanted to talk about fees"), Hook::None);
}

#[test]
fn no_hook_at_all_is_the_first_thing_said() {
    let f = faults(&weak());
    assert_eq!(f[0], Fault::ContextFirst);
    assert!(f.contains(&Fault::SlowStart));
    assert!(f.contains(&Fault::TooGeneral));
}

#[test]
fn the_advice_is_specific_enough_to_act_on() {
    assert!(Fault::SlowStart.fix().contains("2–4 seconds"));
    assert!(Fault::LateValue.fix().contains("at 40 seconds should be at 8"));
    assert!(Fault::TooGeneral.fix().contains("with numbers"));
}

#[test]
fn something_that_opens_well_is_left_alone() {
    let said = before_posting(&strong());
    assert!(said.contains("Opens well"));
    assert!(said.contains("Nothing I'd change"));
}

#[test]
fn views_are_an_outcome_not_a_signal() {
    // 40k views at 8% completion taught you nothing you can repeat.
    let vain = Performance {
        id: "1".into(), views: 40_000, completion: 0.08, held_at_three: 0.35,
        saves: 12, shares: 4, hook: Hook::Question, topic: "fees".into(), seconds: 50.0,
    };
    assert!(!vain.worth_repeating());

    let real = Performance {
        id: "2".into(), views: 3_000, completion: 0.45, held_at_three: 0.72,
        saves: 210, shares: 80, hook: Hook::Contradiction, topic: "fees".into(), seconds: 24.0,
    };
    assert!(real.worth_repeating());
}

#[test]
fn under_eight_posts_any_pattern_is_noise_and_atlas_says_so() {
    let few: Vec<Performance> = (0..4)
        .map(|i| Performance {
            id: i.to_string(), views: 1000, completion: 0.4, held_at_three: 0.7,
            saves: 50, shares: 10, hook: Hook::Contradiction, topic: "fees".into(), seconds: 24.0,
        })
        .collect();
    let l = learn(&few, &ContentConfig::default());
    assert!(!l.confident);
    assert!(how_its_going(&l).contains("anything I said would be noise"));

    // How many is "enough" is yours: `content.min_posts_for_patterns` shipped
    // 8 and was read by nothing, because `learn` hardcoded `>= 8` and `atlas
    // content learn` printed a bare 8 beside it.
    let lower = ContentConfig { min_posts_for_patterns: 4, ..ContentConfig::default() };
    assert!(learn(&few, &lower).confident, "four posts, and four is what was asked for");
    let higher = ContentConfig { min_posts_for_patterns: 40, ..ContentConfig::default() };
    assert!(!learn(&few, &higher).confident);
}

#[test]
fn with_enough_posted_it_names_the_opening_that_works_for_you() {
    let mut history: Vec<Performance> = Vec::new();
    for i in 0..6 {
        history.push(Performance {
            id: format!("c{i}"), views: 5000, completion: 0.4, held_at_three: 0.78,
            saves: 200, shares: 60, hook: Hook::Contradiction, topic: "fees".into(), seconds: 25.0,
        });
    }
    for i in 0..4 {
        history.push(Performance {
            id: format!("q{i}"), views: 5000, completion: 0.1, held_at_three: 0.3,
            saves: 10, shares: 2, hook: Hook::Question, topic: "general".into(), seconds: 55.0,
        });
    }
    let l = learn(&history, &ContentConfig::default());
    assert!(l.confident);
    let said = how_its_going(&l);
    assert!(said.contains("sounds wrong"), "the contradiction hook: {said}");
    assert!(said.contains("78%"));
    assert!(said.contains("seconds"), "and the length that works");
}

#[test]
fn atlas_never_posts_without_you_saying_so() {
    // `content.always_confirm` was a `#[serde(skip)]` bool pinned true that
    // nothing read, and this test asserted it was true -- which it was, the
    // way every constant is. Deleted 19 Sep 2026.
    //
    // What keeps the promise is `publish::Publisher::check`, which every send
    // goes through and which refuses a post whose approval does not match the
    // text it is about to send. Time passing is not consent, and neither is
    // an approval of something else.
    use atlas::publish::{Channel, Publisher, SendCheck};
    let mut pub_ = Publisher::default();
    let id = pub_.draft(Channel::X, "the post as approved");

    assert!(
        matches!(pub_.check(id, true, None), SendCheck::Hold(_)),
        "an unapproved post must not be sendable"
    );

    assert!(pub_.approve(id));
    assert!(matches!(pub_.check(id, true, None), SendCheck::Go), "approved, so it may go");

    // The edit after the approval is the case this exists for.
    assert!(pub_.edit(id, "something else entirely"));
    match pub_.check(id, true, None) {
        SendCheck::Hold(why) => assert!(
            why.contains("changed since you approved"),
            "it has to say why, or the refusal reads as a bug: {why}"
        ),
        SendCheck::Go => panic!("it would have posted text you never approved"),
    }

    // And an old config naming the removed key still loads.
    let parsed: ContentConfig =
        serde_yaml::from_str("enabled: true\nalways_confirm: false\n").unwrap();
    assert!(parsed.enabled, "an unknown key must not stop the section parsing");
}

#[test]
fn the_edits_it_offers_are_the_tedious_half_and_need_no_model() {
    let e = edits_it_can_do();
    assert!(e.iter().any(|(n, _)| n.contains("trim the dead opening")));
    assert!(e.iter().any(|(_, why)| why.contains("most watch on mute")));
    assert!(e.iter().any(|(_, why)| why.contains("15%")), "with the actual gain");
}

#[test]
fn content_work_is_off_until_you_turn_it_on() {
    assert!(!ContentConfig::default().enabled);
}

// ================= a sentence that is already the whole record =================

use atlas::capture::{made, read_spoken};

#[test]
fn a_task_dictated_in_one_breath_arrives_complete() {
    // A note you file later is a debt. An item that arrives complete is done.
    let said = "make a reel in the Homelab project, call it the fee tiers explainer, \
                assign it to me, due Monday, in the script add text on screen";
    let s = read_spoken(said, &["Homelab".into()], &["Priya".into()]);

    assert_eq!(s.title.as_deref(), Some("the fee tiers explainer"));
    assert_eq!(s.project.as_deref(), Some("Homelab"));
    assert_eq!(s.kind.as_deref(), Some("reel"));
    assert_eq!(s.assigned_to.as_deref(), Some("me"));
    assert_eq!(s.due_words.as_deref(), Some("monday"));
    assert!(s.notes.as_deref().unwrap().contains("add text on screen"));
    assert!(s.is_a_whole_item());
}

#[test]
fn a_date_is_kept_as_words_rather_than_guessed_at() {
    // Resolving "Monday" needs today's date, and guessing here is how
    // something lands on the wrong Monday.
    let s = read_spoken("call it the audit, due next Thursday", &[], &[]);
    assert_eq!(s.due_words.as_deref(), Some("next thursday"));
}

#[test]
fn assigning_it_to_someone_by_name_works() {
    let s = read_spoken("call it the cut, assign it to Priya", &[], &["Priya".into()]);
    assert_eq!(s.assigned_to.as_deref(), Some("Priya"));
}

#[test]
fn something_half_said_asks_one_question_not_three() {
    // Asking three questions about a sentence someone said while walking is
    // how you teach them not to bother.
    let vague = read_spoken("make a reel about the fee thing", &[], &[]);
    assert!(!vague.is_a_whole_item());
    assert_eq!(vague.worth_asking(), Some("what should I call it?"));

    let named = read_spoken("make a reel, call it the fee tiers", &[], &[]);
    assert_eq!(named.worth_asking(), Some("which project?"));
}

#[test]
fn a_complete_one_is_asked_nothing() {
    let s = read_spoken("call it the audit in Homelab", &["Homelab".into()], &[]);
    assert!(s.worth_asking().is_none());
}

#[test]
fn what_it_says_back_repeats_only_what_is_worth_getting_wrong() {
    let s = read_spoken(
        "make a reel in Homelab, call it the fee tiers, assign it to me, due Monday",
        &["Homelab".into()],
        &[],
    );
    let said = made(&s);
    assert!(said.contains("the fee tiers"));
    assert!(said.contains("Homelab"));
    assert!(said.contains("monday"));
    assert!(!said.contains("assign"), "not the whole record read back");
    assert!(said.len() < 70, "one line: {said}");
}
