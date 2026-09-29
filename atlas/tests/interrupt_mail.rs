use atlas::interrupt::{mute_from, Decision, Doing, Gate, InterruptConfig, Thing, Weight};
use atlas::mail::{
    action_for, categories, may_touch, rehearsal, trace, what_the_trail_says, Action, MailConfig,
    Provider, Trail,
};
use atlas::prose::{apply_certain, check, Kind, Overreach, ProseConfig, Voice};

fn thing(say: &str, weight: Weight, about: &str, actionable: bool, at: u64) -> Thing {
    Thing { say: say.into(), weight, about: about.into(), actionable, at }
}

fn icfg() -> InterruptConfig {
    InterruptConfig::default()
}

// ================= saying almost nothing =================

#[test]
fn the_test_is_whether_you_would_do_anything_differently() {
    // Not "is it true" or "is it interesting". Almost nothing passes.
    let mut g = Gate::default();
    let idle_fact = thing("the index finished", Weight::WhenYouStop, "index", false, 0);
    match g.consider(&idle_fact, Doing::Between, &icfg(), 0) {
        Decision::Drop(why) => assert!(why.contains("nothing you'd do differently")),
        o => panic!("{o:?}"),
    }
}

#[test]
fn something_urgent_and_actionable_gets_through_anything() {
    let mut g = Gate::default();
    let urgent = thing("the nine o'clock post is over length", Weight::Now, "post", true, 0);
    assert!(matches!(g.consider(&urgent, Doing::Focused, &icfg(), 0), Decision::Say(_)));
}

#[test]
fn nothing_is_said_while_you_are_head_down() {
    let mut g = Gate::default();
    let t = thing("the backup ran", Weight::Today, "backup", true, 0);
    match g.consider(&t, Doing::Focused, &icfg(), 0) {
        Decision::Hold(why) => assert!(why.contains("middle of something")),
        o => panic!("{o:?}"),
    }
    assert_eq!(g.waiting(), 1);
}

#[test]
fn nothing_is_said_into_a_call_or_an_empty_room() {
    let mut g = Gate::default();
    for doing in [Doing::InACall, Doing::Away] {
        let t = thing("x", Weight::Today, &format!("{doing:?}"), true, 0);
        assert!(matches!(g.consider(&t, doing, &icfg(), 0), Decision::Hold(_)));
    }
}

#[test]
fn the_same_kind_of_thing_is_not_said_twice() {
    // The second time is worse than the first, not better.
    let mut g = Gate::default();
    let t = thing("memory is tight", Weight::Today, "memory", true, 0);
    assert!(matches!(g.consider(&t, Doing::Between, &icfg(), 0), Decision::Say(_)));
    match g.consider(&t, Doing::Between, &icfg(), 600) {
        Decision::Drop(why) => assert!(why.contains("recently")),
        o => panic!("{o:?}"),
    }
}

#[test]
fn there_is_a_hard_ceiling_on_interruptions_per_hour() {
    let mut g = Gate::default();
    let mut said = 0;
    for i in 0..12 {
        let t = thing("x", Weight::Today, &format!("thing{i}"), true, i * 60);
        if matches!(g.consider(&t, Doing::Between, &icfg(), i * 60), Decision::Say(_)) {
            said += 1;
        }
    }
    assert_eq!(said, 4, "four an hour is already more than most days need");
}

#[test]
fn talking_to_atlas_is_not_an_interruption_so_the_budget_does_not_apply() {
    let mut g = Gate::default();
    for i in 0..8 {
        let t = thing("x", Weight::Today, &format!("t{i}"), true, 0);
        assert!(matches!(g.consider(&t, Doing::Talking, &icfg(), 0), Decision::Say(_)));
    }
}

#[test]
fn everything_held_comes_back_as_one_thing_not_six() {
    // Six remarks arriving together is still six interruptions.
    let mut g = Gate::default();
    for i in 0..4 {
        let t = thing(&format!("thing {i}."), Weight::Today, &format!("t{i}"), true, 0);
        g.consider(&t, Doing::Focused, &icfg(), 0);
    }
    let said = g.release(100).unwrap();
    assert!(said.contains("And 3 other things"));
    assert_eq!(g.waiting(), 0);
}

#[test]
fn the_most_important_held_thing_leads() {
    let mut g = Gate::default();
    g.consider(&thing("minor.", Weight::WhenYouStop, "a", true, 0), Doing::Away, &icfg(), 0);
    g.consider(&thing("the post is over length.", Weight::Today, "b", true, 0), Doing::Away, &icfg(), 0);
    assert!(g.release(100).unwrap().starts_with("the post is over length."));
}

#[test]
fn something_held_too_long_is_dropped_rather_than_said_late() {
    // An hour-old observation about a stalled job isn't worth saying when you
    // sit back down.
    let mut g = Gate::default();
    g.consider(&thing("x", Weight::Today, "a", true, 0), Doing::Focused, &icfg(), 0);
    assert_eq!(g.forget_stale(7200, 3600), 1);
    assert!(g.release(7200).is_none());
}

#[test]
fn nothing_held_means_nothing_said_when_you_come_back() {
    assert!(Gate::default().release(0).is_none());
}

#[test]
fn you_can_mute_a_whole_kind_of_thing_by_saying_so() {
    assert_eq!(mute_from("stop telling me about the backups"), Some("the backups".into()));
    assert_eq!(mute_from("i don't care about disk space"), Some("disk space".into()));
    assert!(mute_from("what's the weather").is_none());

    let muted = InterruptConfig { muted: vec!["backup".into()], ..icfg() };
    let mut g = Gate::default();
    let t = thing("the backup ran", Weight::Now, "backup", true, 0);
    assert!(matches!(g.consider(&t, Doing::Talking, &muted, 0), Decision::Drop(_)));
}

// ================= grammar that resolves rather than flags =================

fn pcfg() -> ProseConfig {
    ProseConfig { enabled: true, ..Default::default() }
}

#[test]
fn your_going_is_not_ambiguous_and_is_just_fixed() {
    // You cannot own a "going". Flagging this rather than fixing it is the
    // sort of caution that annoys without protecting.
    let text = "your going to need the account number";
    let (out, _) = apply_certain(text, &check(text, &pcfg()));
    assert!(out.starts_with("You're going"), "got: {out}");
}

#[test]
fn youre_before_a_noun_is_fixed_the_other_way() {
    let text = "check you're email for the receipt";
    let (out, _) = apply_certain(text, &check(text, &pcfg()));
    assert!(out.contains("your email"), "got: {out}");
}

#[test]
fn its_is_now_resolved_rather_than_flagged_where_the_next_word_settles_it() {
    let text = "its going to take six weeks";
    let fixes = check(text, &pcfg());
    let its = fixes.iter().find(|f| f.was.to_lowercase() == "its").unwrap();
    assert_eq!(its.kind, Kind::Certain);
    let (out, _) = apply_certain(text, &fixes);
    assert!(out.starts_with("It's going"), "got: {out}");
}

#[test]
fn its_own_is_still_left_completely_alone() {
    let text = "the system found its own way";
    assert!(check(text, &pcfg()).iter().all(|f| f.was.to_lowercase() != "its"));
}

#[test]
fn to_before_a_comparison_becomes_too() {
    let text = "that is to expensive";
    let (out, _) = apply_certain(text, &check(text, &pcfg()));
    assert!(out.contains("too expensive"), "got: {out}");
}

#[test]
fn he_dont_is_always_wrong() {
    let text = "he dont know about it";
    let (out, _) = apply_certain(text, &check(text, &pcfg()));
    assert!(out.contains("doesn't know"), "got: {out}");
}

#[test]
fn changing_something_back_three_times_means_atlas_is_wrong() {
    // The third time, the problem is Atlas.
    let mut o = Overreach::default();
    for _ in 0..3 {
        o.you_undid("asses");
    }
    assert!(o.leave_alone("asses"));
    assert!(o.ask("asses").unwrap().contains("keep changing it back"));
    assert!(!o.leave_alone("teh"));
}

#[test]
fn how_you_write_is_learned_so_your_style_is_not_flagged() {
    let mut v = Voice::default();
    for _ in 0..8 {
        v.learn("And that's the thing. But it works. So we ship it. It's fine.");
    }
    assert!(v.sentences_seen >= 20);
    assert!(v.is_your_style("sentence starts with a conjunction"));
    assert!(v.is_your_style("short sentences"));
}

#[test]
fn nothing_is_assumed_about_your_style_from_two_sentences() {
    let mut v = Voice::default();
    v.learn("And so it goes.");
    assert!(!v.is_your_style("sentence starts with a conjunction"));
}

// ================= your mailbox, whoever provides it =================

#[test]
fn the_provider_is_worked_out_from_the_address() {
    assert_eq!(Provider::from_address("eric@gmail.com"), Provider::Gmail);
    assert_eq!(Provider::from_address("eric@hotmail.com"), Provider::Outlook);
    assert_eq!(Provider::from_address("eric@yahoo.com"), Provider::Yahoo);
    assert_eq!(Provider::from_address("eric@hisowndomain.com"), Provider::Other);
}

#[test]
fn each_provider_says_what_you_actually_have_to_do() {
    for p in [Provider::Gmail, Provider::Yahoo] {
        let how = p.how_to_connect();
        assert!(how.contains("app password"), "{p:?}: {how}");
    }
    // Outlook has no password way in any more; saying otherwise sent people
    // to a settings page that can't help.
    let outlook = Provider::Outlook.how_to_connect();
    assert!(outlook.contains("OAuth") && !outlook.contains("app password"), "{outlook}");
    assert!(Provider::Gmail.how_to_connect().contains("revocable"));
    // Behaviour, not just wording: each provider gives its own steps, not one
    // generic canned line.
    assert_ne!(
        Provider::Gmail.how_to_connect(),
        Provider::Outlook.how_to_connect(),
        "two providers handed back the same instructions"
    );
}

#[test]
fn gmail_labels_are_not_folders_and_atlas_knows_the_difference() {
    // Moving a Gmail message the way you'd move an Outlook one makes mail
    // disappear from the inbox.
    assert!(Provider::Gmail.has_labels());
    assert!(!Provider::Outlook.has_labels());

    let cfg = MailConfig { actually_sort: true, label_only: false, ..Default::default() };
    assert_eq!(action_for("Needs you", Provider::Gmail, &cfg), Action::Label("Needs you".into()));
    assert_eq!(action_for("Needs you", Provider::Outlook, &cfg), Action::MoveTo("Needs you".into()));
}

#[test]
fn nothing_atlas_does_to_your_mail_is_irreversible() {
    for a in [
        Action::Label("x".into()),
        Action::MoveTo("x".into()),
        Action::MarkRead,
        Action::Archive,
    ] {
        assert!(a.reversible());
    }
}

#[test]
fn the_safe_default_labels_without_moving_anything() {
    let cfg = MailConfig { actually_sort: true, ..Default::default() };
    assert!(cfg.label_only);
    assert_eq!(action_for("Noise", Provider::Outlook, &cfg), Action::Label("Noise".into()));
}

#[test]
fn the_first_run_says_what_it_would_do_rather_than_doing_it() {
    let cfg = MailConfig::default();
    assert_eq!(action_for("Noise", Provider::Gmail, &cfg), Action::Nothing);

    let said = rehearsal(&[("Needs you".into(), 3), ("Noise".into(), 40)], &cfg);
    assert!(said.contains("43 messages"));
    assert!(said.contains("Say go"));
    assert!(said.contains("nothing gets deleted"));
    assert!(said.contains("undo any of it in your mail app"));
}

#[test]
fn sent_and_drafts_are_never_touched() {
    let cfg = MailConfig::default();
    for f in ["Sent", "Drafts", "Trash", "Spam"] {
        assert!(!may_touch(f, &cfg), "{f}");
    }
    assert!(may_touch("INBOX", &cfg));
}

#[test]
fn the_categories_make_sense_on_a_phone_not_in_a_config_file() {
    let c = categories();
    assert!(c.iter().any(|(n, _)| *n == "Needs you"));
    assert!(c.iter().any(|(n, _)| *n == "Waiting on them"));
    assert!(c.iter().all(|(n, _)| !n.contains('_')));
}

#[test]
fn where_your_address_leaked_is_traceable_through_plus_addressing() {
    // Unsubscribing one at a time treats the symptom.
    let trails = vec![
        Trail { sender: "Some Store".into(), arrived_at: Some("eric+store@gmail.com".into()), since: 0 },
        Trail { sender: "Unrelated Spam".into(), arrived_at: Some("eric+store@gmail.com".into()), since: 0 },
        Trail { sender: "More Spam".into(), arrived_at: Some("eric+store@gmail.com".into()), since: 0 },
    ];
    let groups = trace(&trails);
    assert_eq!(groups.len(), 1);
    let said = what_the_trail_says(&groups[0].0, &groups[0].1);
    assert!(said.contains("3 different senders"));
    assert!(said.contains("Worth retiring the alias"));
}

#[test]
fn mail_access_is_off_until_you_turn_it_on() {
    assert!(!MailConfig::default().enabled);
    assert!(!MailConfig::default().actually_sort);
}
