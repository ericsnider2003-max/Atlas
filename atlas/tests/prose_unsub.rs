use atlas::prose::{
    apply_certain, check, check_phrases, may_correct_in, spoken as prose_spoken, Kind, ProseConfig,
};
use atlas::unsub::{judge, one_click, plan, spoken as unsub_spoken, Sender, UnsubConfig, Verdict};

fn cfg() -> ProseConfig {
    ProseConfig { enabled: true, ..Default::default() }
}

// ================= fixing what you type =================

#[test]
fn a_dropped_apostrophe_has_one_possible_fix_so_it_is_just_fixed() {
    let (out, n) = {
        let text = "i dont think that wont work";
        let fixes = check(text, &cfg());
        apply_certain(text, &fixes)
    };
    assert!(out.contains("don't") && out.contains("won't"));
    assert!(out.starts_with('I'), "and \"i\" is always \"I\": {out}");
    assert!(n >= 3);
}

#[test]
fn a_doubled_word_is_removed() {
    let text = "this is is a problem";
    let (out, _) = apply_certain(text, &check(text, &cfg()));
    assert_eq!(out, "This is a problem", "and the capital comes along too");
}

#[test]
fn typos_you_actually_make_are_caught() {
    for (wrong, right) in [
        ("teh", "the"), ("recieve", "receive"), ("seperate", "separate"),
        ("definately", "definitely"), ("diagnos", "diagnose"), ("alot", "a lot"),
    ] {
        let text = format!("we should {wrong} it");
        let (out, _) = apply_certain(&text, &check(&text, &cfg()));
        assert!(out.contains(right), "{wrong} should become {right}: {out}");
    }
}

#[test]
fn a_lower_case_start_after_a_full_stop_is_capitalised() {
    let text = "that works. now for the next one.";
    let (out, _) = apply_certain(text, &check(text, &cfg()));
    assert!(out.starts_with("That works. Now"), "got: {out}");
}

#[test]
fn capitalisation_is_kept_when_fixing() {
    let text = "Dont do that.";
    let (out, _) = apply_certain(text, &check(text, &cfg()));
    assert!(out.starts_with("Don't"), "got: {out}");
}

#[test]
fn something_the_next_word_cannot_settle_is_flagged_not_changed() {
    // "its going" is now resolved — you can't own a "going". But whether a
    // change "affects" or "effects" something needs the sentence, not the
    // next word, so it stays a flag.
    let text = "the delay will affect the launch";
    let fixes = check(text, &cfg());
    let f = fixes.iter().find(|f| f.was == "affect").expect("should notice");
    assert_ne!(f.kind, Kind::Certain, "never silently changed");
    let (out, _) = apply_certain(text, &fixes);
    assert!(out.contains("affect"), "left alone: {out}");
}

#[test]
fn its_before_a_possessive_word_is_left_entirely_alone() {
    let text = "the system found its own way";
    let fixes = check(text, &cfg());
    assert!(!fixes.iter().any(|f| f.was == "its"), "\"its own\" is correct");
}

#[test]
fn a_comparison_wanting_than_is_caught_by_reading_the_phrase() {
    // The word-by-word pass can't see this.
    let fixes = check_phrases("this is better then the last one");
    assert_eq!(fixes.len(), 1);
    assert_eq!(fixes[0].becomes, "than");
    assert!(fixes[0].because.contains("comparing"));

    assert!(check_phrases("first we do this then that").is_empty(), "\"then\" is fine there");
}

#[test]
fn your_own_words_are_not_mistakes() {
    let mine = ProseConfig { my_words: vec!["asses".into()], ..cfg() };
    let text = "the asses in the field";
    assert!(check(text, &mine).iter().all(|f| f.was != "asses"));
}

#[test]
fn nothing_is_corrected_in_code_or_a_terminal() {
    // "dont" in a string literal is meant to be there.
    let c = cfg();
    assert!(!may_correct_in("Visual Studio Code", &c));
    assert!(!may_correct_in("Windows PowerShell", &c));
    assert!(!may_correct_in("1Password", &c), "and never near a password field");
    assert!(may_correct_in("Notepad", &c));
    assert!(may_correct_in("Outlook", &c));
}

#[test]
fn atlas_says_nothing_about_what_it_quietly_fixed() {
    // You don't want a notification every time you type "dont".
    let text = "i dont think so";
    let fixes = check(text, &cfg());
    let (_, n) = apply_certain(text, &fixes);
    assert!(n > 0);
    assert_eq!(prose_spoken(&fixes, n), "", "silence is the right answer");
}

#[test]
fn it_does_mention_the_ones_it_did_not_touch() {
    let text = "that will affect the timing";
    let fixes = check(text, &cfg());
    let said = prose_spoken(&fixes, 1);
    assert!(said.contains("affect"), "got: {said}");
}

#[test]
fn correcting_is_off_until_you_turn_it_on() {
    assert!(!ProseConfig::default().enabled);
}

// ================= clearing out a personal inbox =================

fn ucfg() -> UnsubConfig {
    UnsubConfig { enabled: true, ..Default::default() }
}

fn newsletter() -> Sender {
    Sender {
        address: "hello@somestore.com".into(),
        name: "Some Store".into(),
        count: 40,
        opened: 1,
        replied: 0,
        list_unsubscribe: Some("<https://somestore.com/unsub?id=abc>".into()),
        last_seen_days: 2,
    }
}

fn spam() -> Sender {
    Sender {
        address: "winner@x8f2.biz".into(),
        name: "You've Won".into(),
        count: 30,
        opened: 0,
        replied: 0,
        list_unsubscribe: None,
        last_seen_days: 1,
    }
}

#[test]
fn a_legitimate_sender_you_never_read_gets_a_proper_unsubscribe() {
    match judge(&newsletter(), &ucfg()) {
        Verdict::Unsubscribe { how, why } => {
            assert!(how.contains("https://"));
            assert!(why.contains("proper unsubscribe"));
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn spam_is_blocked_and_never_unsubscribed_from() {
    // Clicking unsubscribe in spam confirms your address is real and read.
    // It reliably makes things worse.
    match judge(&spam(), &ucfg()) {
        Verdict::BlockOnly { why } => {
            assert!(why.contains("confirms your address is real"), "got: {why}")
        }
        o => panic!("expected block, got {o:?}"),
    }
}

#[test]
fn a_header_that_is_not_the_real_standard_is_not_trusted() {
    let dodgy = Sender {
        list_unsubscribe: Some("<http://tracking.x8f2.biz/click?id=you>".into()),
        ..spam()
    };
    assert!(!dodgy.has_safe_exit(), "plain http and a tracking path");
    assert!(matches!(judge(&dodgy, &ucfg()), Verdict::BlockOnly { .. }));
}

#[test]
fn something_you_actually_read_is_kept() {
    let read = Sender { opened: 30, ..newsletter() };
    match judge(&read, &ucfg()) {
        Verdict::Keep { why } => assert!(why.contains("you open about")),
        o => panic!("{o:?}"),
    }
}

#[test]
fn some_senders_are_kept_whatever_the_numbers_say() {
    // Missing one of these costs you something the numbers don't capture.
    for name in ["Chase Bank", "IRS", "State Farm Insurance", "your broker", "Invoice from X"] {
        let s = Sender { name: name.into(), opened: 0, ..newsletter() };
        assert!(matches!(judge(&s, &ucfg()), Verdict::Keep { .. }), "{name} should be kept");
    }
}

#[test]
fn a_sender_with_only_a_couple_of_messages_is_not_judged_yet() {
    let new = Sender { count: 2, opened: 0, ..newsletter() };
    assert!(matches!(judge(&new, &ucfg()), Verdict::Keep { .. }));
}

#[test]
fn the_plan_separates_the_two_kinds_and_says_why() {
    let c = plan(&[newsletter(), spam(), Sender { opened: 25, ..newsletter() }], &ucfg());
    assert_eq!(c.unsubscribe.len(), 1);
    assert_eq!(c.block.len(), 1);
    assert_eq!(c.keep, 1);

    let said = unsub_spoken(&c);
    assert!(said.contains("1 I can unsubscribe from properly"));
    assert!(said.contains("clicking those would confirm your address"));
}

#[test]
fn it_says_how_much_quieter_the_year_gets() {
    let said = unsub_spoken(&plan(&[newsletter(), spam()], &ucfg()));
    assert!(said.contains("fewer a year"), "got: {said}");
}

#[test]
fn a_clean_inbox_gets_left_alone() {
    let good = Sender { opened: 35, ..newsletter() };
    assert!(unsub_spoken(&plan(&[good], &ucfg())).contains("Nothing worth clearing out"));
}

#[test]
fn one_click_unsubscribe_posts_rather_than_opening_a_browser() {
    // A browser visit is what loads their tracking.
    let (url, body) = one_click("<https://somestore.com/unsub?id=abc>").unwrap();
    assert_eq!(url, "https://somestore.com/unsub?id=abc");
    assert_eq!(body, "List-Unsubscribe=One-Click");

    let (mail, body) = one_click("<mailto:unsub@somestore.com>").unwrap();
    assert!(mail.starts_with("mailto:"));
    assert!(body.is_empty());

    assert!(one_click("click here to unsubscribe").is_none(), "not a header, not used");
}

#[test]
fn clearing_out_is_off_until_you_turn_it_on() {
    assert!(!UnsubConfig::default().enabled);
}
