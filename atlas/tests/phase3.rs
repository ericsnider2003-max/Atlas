use atlas::draft::{critique, improved, revision_brief, spoken as draft_spoken, DraftConfig, Fault};
use atlas::learned::{spoken as learned_spoken, Advice, Cause, Learned};
use atlas::timebox::{size_of, Box_, Size, State, Stopped};

// ================= being honest about a draft =================

fn cfg() -> DraftConfig {
    DraftConfig::default()
}

#[test]
fn throat_clearing_at_the_start_is_caught() {
    let n = critique("In today's world, latency matters. We cut it to 300ms.", None, &cfg());
    assert!(n.iter().any(|n| n.fault == Fault::ThroatClearing));
}

#[test]
fn one_hedge_is_fine_and_three_is_a_refusal_to_commit() {
    assert!(critique("I think this is the right call.", None, &cfg()).is_empty());
    let n = critique(
        "I think this is perhaps somewhat the right call, maybe.",
        None,
        &cfg(),
    );
    assert!(n.iter().any(|n| n.fault == Fault::Hedging));
}

#[test]
fn words_that_sound_like_effort_and_mean_nothing_are_named() {
    let n = critique("We leverage a robust, best-in-class approach.", None, &cfg());
    let f = n.iter().find(|n| n.fault == Fault::Filler).unwrap();
    assert!(f.evidence.contains("leverage"));
    assert!(f.evidence.contains("robust"), "and it lists them: {}", f.evidence);
}

#[test]
fn saying_the_same_thing_twice_is_caught() {
    let text = "The endpointing change cuts the recording short when you stop talking. \
                Recording stops short when you finish talking, thanks to endpointing.";
    let n = critique(text, None, &cfg());
    let r = n.iter().find(|n| n.fault == Fault::Repetition).expect("should spot it");
    assert!(r.evidence.contains("and"), "it quotes both: {}", r.evidence);
}

#[test]
fn a_question_tacked_on_the_end_is_caught() {
    let n = critique("We cut latency to 300ms. Thoughts?", None, &cfg());
    assert!(n.iter().any(|n| n.fault == Fault::TackedOnQuestion));
}

#[test]
fn a_real_question_in_the_middle_is_not() {
    let n = critique("What actually costs the time? Transcription, mostly. We cut it to 300ms.", None, &cfg());
    assert!(!n.iter().any(|n| n.fault == Fault::TackedOnQuestion));
}

#[test]
fn something_with_nothing_specific_in_it_is_flagged() {
    let vague = "This approach delivers value across the board and improves outcomes \
                 for everyone involved, making things better in a number of important ways \
                 that really do matter quite a lot when you consider it properly overall.";
    assert!(critique(vague, None, &cfg()).iter().any(|n| n.fault == Fault::Vague));
}

#[test]
fn a_draft_with_numbers_and_names_is_not_called_vague() {
    let concrete = "Atlas cut the turn from 8 seconds to 300ms by watching for silence. \
                    Whisper transcribes a third of the audio it used to, on the same laptop, \
                    with no change to the model at all and no extra memory.";
    assert!(!critique(concrete, None, &cfg()).iter().any(|n| n.fault == Fault::Vague));
}

#[test]
fn every_sentence_being_the_same_length_reads_as_machine_output() {
    let flat = "The system works well today. The users like the new features. \
                The team shipped it last week. The results have been quite good.";
    assert!(critique(flat, None, &cfg()).iter().any(|n| n.fault == Fault::Monotonous));
}

#[test]
fn over_the_limit_says_by_how_much() {
    let n = critique(&"x".repeat(300), Some(280), &cfg());
    let t = n.iter().find(|n| n.fault == Fault::TooLong).unwrap();
    assert!(t.evidence.contains("300 characters") && t.evidence.contains("280"));
}

#[test]
fn a_good_draft_gets_left_alone() {
    let good = "Atlas now stops listening when you stop talking. On a laptop with 15GB of \
                shared memory that halves what Whisper has to transcribe. It cost nothing.";
    assert!(critique(good, None, &cfg()).is_empty());
    assert_eq!(draft_spoken(&[]), "That reads well. Nothing I'd change.");
}

#[test]
fn the_rewrite_instruction_is_specific_rather_than_make_it_better() {
    let n = critique("In today's world, we leverage robust solutions.", None, &cfg());
    let brief = revision_brief(&n).unwrap();
    assert!(brief.contains("opens with a sentence that says nothing"));
    assert!(brief.contains("leverage"));
    assert!(brief.contains("Don't add anything new"));
}

#[test]
fn nothing_wrong_means_no_rewrite_at_all() {
    // A rewrite with nothing to fix makes it worse.
    assert!(revision_brief(&[]).is_none());
}

#[test]
fn a_rewrite_that_introduces_more_faults_is_not_an_improvement() {
    let before = critique("In today's world, this is fine.", None, &cfg());
    let after = critique("In today's world, we leverage robust seamless synergy.", None, &cfg());
    assert!(!improved(&before, &after), "fixing one and adding three is not better");
}

#[test]
fn you_can_switch_off_a_fault_you_disagree_with() {
    let ignore = DraftConfig { ignore: vec![Fault::Hedging], ..cfg() };
    let text = "I think this is perhaps somewhat right, maybe.";
    assert!(!critique(text, None, &ignore).iter().any(|n| n.fault == Fault::Hedging));
}

// ================= remembering what didn't work =================

#[test]
fn something_never_tried_just_goes_ahead() {
    assert_eq!(Learned::default().advise("click the export button", "acme.com", 0), Advice::Fresh);
}

#[test]
fn failing_once_because_of_the_world_is_worth_another_go() {
    // Once is bad luck.
    let mut l = Learned::default();
    l.record("click export", "acme.com", Cause::Outside, "the site was down", 0);
    match l.advise("click export", "acme.com", 100) {
        Advice::TryAgain { because } => assert!(because.contains("may have changed")),
        o => panic!("{o:?}"),
    }
}

#[test]
fn failing_twice_stops_it_and_says_how_many_times() {
    // "We've tried that twice" is an argument. "That won't work" is an
    // assertion.
    let mut l = Learned::default();
    l.record("click export", "acme.com", Cause::Outside, "timed out", 0);
    l.record("click export", "acme.com", Cause::Outside, "timed out again", 100);
    match l.advise("click export", "acme.com", 200) {
        Advice::Dont { because, .. } => assert!(because.contains("tried 2 times"), "got: {because}"),
        o => panic!("{o:?}"),
    }
}

#[test]
fn an_approach_that_was_never_going_to_work_is_refused_the_first_time() {
    let mut l = Learned::default();
    l.record("scrape the PDF as text", "the bank", Cause::WrongIdea, "it's a scan, not text", 0);
    match l.advise("scrape the PDF as text", "the bank", 100) {
        Advice::Dont { because, .. } => assert!(because.contains("doesn't work here")),
        o => panic!("{o:?}"),
    }
    // Behaviour, not just wording: a wrong idea is refused on its first
    // record, where a one-off world failure gets a second go instead.
    l.record("retry the download", "the bank", Cause::Outside, "the site was down", 0);
    assert!(
        matches!(l.advise("retry the download", "the bank", 100), Advice::TryAgain { .. }),
        "a single world failure was refused like a wrong idea"
    );
}

#[test]
fn it_suggests_something_that_worked_in_the_same_place() {
    let mut l = Learned::default();
    l.record("the export button", "acme.com", Cause::WrongIdea, "there isn't one", 0);
    l.record("the download link", "acme.com", Cause::Outside, "slow but fine", 0);
    match l.advise("the export button", "acme.com", 100) {
        Advice::Dont { suggest, .. } => assert_eq!(suggest.as_deref(), Some("the download link")),
        o => panic!("{o:?}"),
    }
}

#[test]
fn a_lesson_about_the_world_expires_because_the_world_moves() {
    // A site that blocked automation in March may not in July.
    let mut l = Learned::default();
    l.record("click export", "acme.com", Cause::Outside, "blocked", 0);
    let three_months = 90 * 86_400;
    assert_eq!(l.advise("click export", "acme.com", three_months), Advice::Fresh);
    assert_eq!(l.forget_stale(three_months), 1);
}

#[test]
fn a_wrong_idea_stays_wrong_for_years() {
    let mut l = Learned::default();
    l.record("scrape the scan", "the bank", Cause::WrongIdea, "it's an image", 0);
    let a_year = 365 * 86_400;
    assert!(matches!(l.advise("scrape the scan", "the bank", a_year), Advice::Dont { .. }));
}

#[test]
fn you_can_ask_what_has_been_tried_somewhere() {
    let mut l = Learned::default();
    l.record("a", "acme.com", Cause::Outside, "x", 0);
    l.record("b", "acme.com", Cause::MyFault, "y", 0);
    l.record("c", "other.com", Cause::Outside, "z", 0);
    assert_eq!(l.about("acme.com", 100).len(), 2);
}

#[test]
fn a_fresh_approach_produces_no_commentary() {
    assert_eq!(learned_spoken(&Advice::Fresh), "");
}

// ================= stopping before you have to ask =================

#[test]
fn a_long_job_says_up_front_how_long_it_will_take() {
    assert_eq!(Size::Quick.warn_up_front(), None, "no commentary on something instant");
    assert_eq!(Size::Long.warn_up_front(), Some("about ten minutes"));
}

#[test]
fn how_big_a_job_is_is_guessed_from_what_you_asked() {
    assert_eq!(size_of("research the QUIC v1 spec"), Size::Long);
    assert_eq!(size_of("summarise this"), Size::Small);
    assert_eq!(size_of("open chrome"), Size::Quick);
    assert_eq!(size_of("work on it overnight"), Size::Open);
}

#[test]
fn going_over_budget_asks_rather_than_killing_the_work() {
    let mut b = Box_::start("research the spec", Size::Small, 0);
    b.progress("found three pages", 190);
    match b.check(200) {
        State::Overrunning { by_secs } => assert_eq!(*by_secs, 80),
        o => panic!("{o:?}"),
    }
    assert!(b.spoken(200).contains("Keep going?"));
}

#[test]
fn stopping_dead_is_caught_earlier_than_running_out_of_time() {
    // Stuck is different from slow.
    let mut b = Box_::start("the download", Size::Long, 0);
    b.progress("started", 0);
    // A ten-minute job that goes quiet for five minutes is stuck.
    match b.check(320) {
        State::Stopped(Stopped::Stalled { since_secs }) => assert_eq!(*since_secs, 320),
        o => panic!("{o:?}"),
    }
    assert!(b.spoken(320).contains("stopped rather than wait it out"));
}

#[test]
fn steady_progress_is_never_interrupted() {
    let mut b = Box_::start("the download", Size::Long, 0);
    for t in (0..500).step_by(20) {
        b.progress("still going", t);
        assert_eq!(*b.check(t), State::Running, "at {t}s");
    }
}

#[test]
fn open_ended_work_is_not_called_stalled() {
    // You said it can take as long as it takes.
    let mut b = Box_::start("overnight work", Size::Open, 0);
    b.progress("started", 0);
    assert_eq!(*b.check(600), State::Running);
}

#[test]
fn finishing_says_so_and_nothing_more() {
    let mut b = Box_::start("the summary", Size::Small, 0);
    b.finish("done");
    assert_eq!(b.spoken(10), "The summary done.");
}

#[test]
fn you_stopping_it_produces_no_speech() {
    let mut b = Box_::start("x", Size::Small, 0);
    b.stop();
    assert_eq!(b.spoken(10), "");
}
