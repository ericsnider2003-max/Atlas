use atlas::wanted::{
    answer_to_ask, ask_which, decide, heard, is_empty_sympathy, read, then_offer, Preferences,
    Wanted, STILL_HONEST,
};

// ================= being told outright =================

#[test]
fn saying_just_listen_settles_it() {
    // Nothing beats being told.
    let r = read("I just need to vent for a minute, don't fix it");
    assert_eq!(r.wanted, Wanted::Hearing);
    assert!(r.confidence > 0.9);
    assert!(r.because.contains("you said"), "and it says why: {}", r.because);
}

#[test]
fn asking_a_direct_question_settles_it_the_other_way() {
    let r = read("what should I do about the deadline");
    assert_eq!(r.wanted, Wanted::Solutions);
    assert!(r.because.contains("you asked"));
}

#[test]
fn being_told_beats_every_other_signal() {
    // Even wrapped in a long vent with a question mark in it.
    let r = read(
        "I can't believe this happened again, every time I get close something breaks, and \
         on top of that the deadline moved — I'm not looking for advice, is that mad?",
    );
    assert_eq!(r.wanted, Wanted::Hearing);
}

// ================= reading it from how you said it =================

#[test]
fn a_short_question_wants_an_answer() {
    assert_eq!(read("Should I move the deadline?").wanted, Wanted::Solutions);
}

#[test]
fn a_long_stretch_with_no_question_is_someone_working_it_out() {
    let vent = "The broker came back and said six weeks for certification, which means nothing \
                goes live before November. I can't believe it's slipped again. Every time I get \
                close to something being finished it moves. And then the VPS bill went up on \
                top of that, which is small but it's the principle of it more than anything.";
    let r = read(vent);
    assert_eq!(r.wanted, Wanted::Hearing);
    assert!(r.because.contains("getting it out rather than asking"), "got: {}", r.because);
}

#[test]
fn a_long_factual_account_with_no_question_leans_to_both() {
    // Told a lot, asked nothing, no signs of venting — hear it, then offer.
    let account = "The certification takes six weeks from submission. The paperwork needs the \
                   entity details and two years of statements. Their compliance team reviews it \
                   in batches on Fridays, so submitting on a Monday buys nothing. The fee is \
                   the same either way and it isn't refundable if they reject it.";
    assert_eq!(read(account).wanted, Wanted::Both);
}

#[test]
fn a_rhetorical_question_is_not_a_question() {
    // "Right?" at the end isn't asking anything.
    let r = read("Every time I get close it slips, again, right?");
    assert_ne!(r.wanted, Wanted::Solutions);
}

#[test]
fn something_short_and_flat_is_genuinely_ambiguous() {
    let r = read("The certification is going to take six weeks.");
    assert_eq!(r.wanted, Wanted::Unclear);
    assert!(r.confidence < 0.5);
}

// ================= asking rather than guessing =================

#[test]
fn when_it_cannot_tell_it_asks_in_one_short_sentence() {
    // Asking costs one sentence. Guessing wrong costs the conversation.
    let q = ask_which();
    assert!(q.contains("think about it with you, or just listen"));
    assert!(q.len() < 70, "a long careful question is its own imposition: {q}");
}

#[test]
fn your_answer_to_that_is_understood_however_you_put_it() {
    assert_eq!(answer_to_ask("just listen"), Some(Wanted::Hearing));
    assert_eq!(answer_to_ask("nothing, I'm fine"), Some(Wanted::Hearing));
    assert_eq!(answer_to_ask("yeah give me ideas"), Some(Wanted::Solutions));
    assert_eq!(answer_to_ask("both I guess"), Some(Wanted::Both));
    assert_eq!(answer_to_ask("mm"), None);
}

#[test]
fn an_ambiguous_message_produces_the_question() {
    let (w, q) = decide("The certification is going to take six weeks.", "homelab", &Preferences::default());
    assert_eq!(w, Wanted::Unclear);
    assert!(q.is_some());
}

// ================= learning which you usually want =================

#[test]
fn after_a_few_times_it_stops_asking_about_the_same_topic() {
    let mut p = Preferences::default();
    for _ in 0..3 {
        p.note("homelab", Wanted::Hearing);
    }
    let (w, q) = decide("The certification is going to take six weeks.", "homelab", &p);
    assert_eq!(w, Wanted::Hearing);
    assert!(q.is_none(), "no need to ask again");
}

#[test]
fn twice_is_not_a_pattern() {
    let mut p = Preferences::default();
    p.note("homelab", Wanted::Hearing);
    p.note("homelab", Wanted::Hearing);
    assert!(p.usual("homelab").is_none());
}

#[test]
fn what_you_said_this_time_beats_what_you_usually_want() {
    let mut p = Preferences::default();
    for _ in 0..5 {
        p.note("homelab", Wanted::Hearing);
    }
    let (w, _) = decide("what should I do about the certification", "homelab", &p);
    assert_eq!(w, Wanted::Solutions, "you asked, so answer");
}

#[test]
fn a_topic_it_has_no_history_with_still_gets_asked_about() {
    let mut p = Preferences::default();
    for _ in 0..5 {
        p.note("homelab", Wanted::Hearing);
    }
    let (_, q) = decide("The oven is broken again.", "house", &p);
    assert!(q.is_some(), "different topic, no assumption");
}

// ================= what it actually says =================

#[test]
fn being_heard_means_being_understood_not_sympathised_with() {
    // Accurate understanding is the thing. "That sounds hard" is what a system
    // says when it has understood nothing.
    let said = heard("six weeks of certification puts the launch past November, and this is the \
                      second slip");
    assert!(said.contains("second slip"));
    assert!(!is_empty_sympathy(&said));
}

#[test]
fn sympathy_noises_are_recognised_as_saying_nothing() {
    assert!(is_empty_sympathy("That sounds really hard, I'm so sorry to hear that."));
    assert!(is_empty_sympathy("I can only imagine. You've got this!"));
    assert!(!is_empty_sympathy("So — six weeks pushes it past November, and that's the second slip."));
}

#[test]
fn the_offer_comes_after_and_is_an_offer() {
    // Following "I hear you" with three bullet points is not listening with
    // extra steps.
    let o = then_offer();
    assert!(o.contains("Want me to"));
    assert!(o.contains("or leave it there"));
}

#[test]
fn listening_is_not_agreeing_and_atlas_says_so() {
    // A system that only reflects you back is worse than useless on the day
    // you need telling.
    assert!(STILL_HONEST.contains("doesn't mean agreeing"));
    assert!(STILL_HONEST.contains("I'll say so"));
    assert!(STILL_HONEST.contains("not while you're still saying it"), "timing, not silence");
}
