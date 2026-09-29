use atlas::attention::{hear, Attention, Heard};
use atlas::certainty::{assess, phrase, CertaintyConfig, Confidence, Grounding};
use atlas::subject::{confirm, resolve, Candidates, Resolution, Subject};
use atlas::system::{describe, judge, reversibility, Change, SystemConfig, Undo, Verdict};
use atlas::policy::Decision;

// ================= what is "this"? =================

fn nothing() -> Candidates {
    Candidates::default()
}

#[test]
fn selecting_something_beats_whatever_is_on_the_clipboard() {
    // Highlighting is the most explicit act there is. The clipboard might
    // hold a password you copied an hour ago.
    let c = Candidates {
        selection: Some("the highlighted paragraph".into()),
        clipboard: Some("something copied ages ago".into()),
        ..nothing()
    };
    match resolve("explain this", &c) {
        Resolution::Found { subject, .. } => {
            assert!(matches!(subject, Subject::Selection(_)), "got {subject:?}")
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn an_empty_clipboard_falls_through_to_what_you_are_looking_at() {
    // The case you raised: "explain this" with nothing copied should not be
    // answered with "there's nothing on the clipboard".
    let c = Candidates {
        clipboard: Some("   ".into()),
        focused_app: Some("chrome".into()),
        focused_title: Some("IETF QUIC v1 specification".into()),
        dwell_secs: 20,
        ..nothing()
    };
    match resolve("explain this", &c) {
        Resolution::Found { subject, .. } => {
            assert_eq!(subject.name(), "IETF QUIC v1 specification in chrome")
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn a_file_you_just_saved_is_a_strong_candidate() {
    let c = Candidates {
        recent_file: Some(("C:/Users/x/Documents/report.pdf".into(), 30)),
        ..nothing()
    };
    match resolve("summarise this document", &c) {
        Resolution::Found { subject, .. } => assert_eq!(subject.name(), "report.pdf"),
        o => panic!("{o:?}"),
    }
}

#[test]
fn a_file_from_yesterday_is_a_weak_one() {
    let recent = Candidates {
        recent_file: Some(("/x/a.pdf".into(), 30)),
        focused_app: Some("chrome".into()),
        dwell_secs: 3000,
        ..nothing()
    };
    let stale = Candidates {
        recent_file: Some(("/x/a.pdf".into(), 90_000)),
        focused_app: Some("chrome".into()),
        dwell_secs: 3000,
        ..nothing()
    };
    let pick = |c: &Candidates| match resolve("explain this", c) {
        Resolution::Found { subject, .. } => Some(subject),
        _ => None,
    };
    assert!(matches!(pick(&recent), Some(Subject::File(_))));
    assert!(!matches!(pick(&stale), Some(Subject::File(_))), "an old file is not what you meant");
}

#[test]
fn the_wording_steers_which_kind_of_thing_is_meant() {
    let c = Candidates {
        clipboard: Some("some text".into()),
        focused_app: Some("chrome".into()),
        focused_title: Some("a page".into()),
        dwell_secs: 10,
        ..nothing()
    };
    match resolve("what's on my screen", &c) {
        Resolution::Found { subject, .. } => {
            assert!(matches!(subject, Subject::Window { .. }), "got {subject:?}")
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn two_equally_likely_things_produce_a_question_not_a_guess() {
    let c = Candidates {
        clipboard: Some("copied text".into()),
        recent_file: Some(("/x/report.pdf".into(), 10)),
        ..nothing()
    };
    // A neutral instruction, so nothing steers it toward one kind.
    match resolve("what is this", &c) {
        Resolution::Ambiguous { question, options } => {
            assert_eq!(options.len(), 2);
            assert!(question.contains("or"), "got: {question}");
        }
        o => panic!("expected a question, got {o:?}"),
    }
}

#[test]
fn with_genuinely_nothing_to_point_at_it_says_so() {
    match resolve("explain this", &nothing()) {
        Resolution::Nothing(why) => assert!(why.contains("nothing copied")),
        o => panic!("{o:?}"),
    }
}

#[test]
fn atlas_names_what_it_took_so_a_wrong_guess_is_obvious_immediately() {
    let c = Candidates { clipboard: Some("x".into()), ..nothing() };
    let said = confirm(&resolve("explain this", &c));
    assert!(said.contains("what you copied"));
    assert!(said.contains("you copied it"), "and why: {said}");
}

// ================= changing the machine =================

fn on() -> SystemConfig {
    SystemConfig { enabled: true, file_roots: vec!["/home/user".into()], ..Default::default() }
}

#[test]
fn a_wallpaper_is_nothing_and_is_treated_as_nothing() {
    let c = Change::Wallpaper { path: "/anywhere/nice.jpg".into() };
    assert_eq!(reversibility(&c), Undo::Instant);
    match judge(&c, &on()) {
        Verdict::Go { decision, .. } => assert_eq!(decision, Decision::ProceedAndReport),
        o => panic!("{o:?}"),
    }
    assert!(describe(&c, Undo::Instant).contains("Say undo"));
}

#[test]
fn moving_a_file_is_done_and_reported_because_it_can_be_undone() {
    let c = Change::MoveFile { from: "/home/user/a.pdf".into(), to: "/home/user/docs/a.pdf".into() };
    assert_eq!(reversibility(&c), Undo::FromTrash);
    match judge(&c, &on()) {
        Verdict::Go { decision, .. } => assert_eq!(decision, Decision::ProceedAndReport),
        o => panic!("{o:?}"),
    }
    assert!(describe(&c, Undo::FromTrash).contains("recoverable"));
}

#[test]
fn files_outside_the_folders_atlas_works_in_are_refused() {
    let c = Change::MoveFile { from: "/etc/passwd".into(), to: "/home/user/x".into() };
    assert!(matches!(judge(&c, &on()), Verdict::Refuse(_)));
}

#[test]
fn security_settings_are_never_touched_however_they_are_asked_for() {
    // Not because they're hard. A voice assistant that can turn off your
    // firewall is a worse thing to own than one that can't.
    for name in [
        "firewall", "Windows Defender", "Smart App Control", "UAC",
        "BitLocker encryption", "account password", "DNS server", "registry",
    ] {
        let c = Change::Setting { name: name.into(), value: "off".into() };
        match judge(&c, &on()) {
            Verdict::Refuse(why) => assert!(why.contains("open the settings page"), "{name}: {why}"),
            o => panic!("{name} should have been refused, got {o:?}"),
        }
    }
}

#[test]
fn appearance_settings_are_allowed_but_asked_about_first() {
    let c = Change::Setting { name: "dark mode".into(), value: "on".into() };
    match judge(&c, &on()) {
        Verdict::Go { decision, undo } => {
            assert_eq!(decision, Decision::RequireApproval, "you'd have to undo it by hand");
            assert_eq!(undo, Undo::ByHand);
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn an_unrecognised_setting_fails_closed() {
    let c = Change::Setting { name: "some obscure toggle".into(), value: "x".into() };
    match judge(&c, &on()) {
        Verdict::Refuse(why) => assert!(why.contains("only change appearance")),
        o => panic!("{o:?}"),
    }
}

#[test]
fn emptying_the_trash_is_the_one_thing_that_cannot_be_taken_back() {
    assert_eq!(reversibility(&Change::PurgeTrash), Undo::Never);
    match judge(&Change::PurgeTrash, &on()) {
        Verdict::Go { decision, .. } => assert_eq!(decision, Decision::RequireApproval),
        o => panic!("{o:?}"),
    }
    assert!(describe(&Change::PurgeTrash, Undo::Never).contains("can't be undone"));
}

#[test]
fn atlas_may_set_up_what_it_needs_for_itself() {
    let c = Change::ForItself { what: "made my own data folder".into(), reversible: true };
    match judge(&c, &on()) {
        Verdict::Go { decision, .. } => assert_eq!(decision, Decision::ProceedAndReport),
        o => panic!("{o:?}"),
    }
}

#[test]
fn with_the_feature_off_nothing_is_changed_at_all() {
    let c = Change::Wallpaper { path: "/x.jpg".into() };
    assert!(matches!(judge(&c, &SystemConfig::default()), Verdict::Refuse(_)));
}

#[test]
fn every_change_says_how_to_take_it_back() {
    for (c, u) in [
        (Change::Wallpaper { path: "/x.jpg".into() }, Undo::Instant),
        (Change::Delete { path: "/home/user/a".into() }, Undo::FromTrash),
        (Change::Setting { name: "theme".into(), value: "dark".into() }, Undo::ByHand),
    ] {
        let said = describe(&c, u);
        assert!(said.len() > 10, "\"done\" on its own leaves you wondering what changed");
    }
}

// ================= the panic word =================

#[test]
fn a_panic_word_is_heard_and_is_not_the_same_as_stop() {
    assert_eq!(hear("stop everything"), Some(Heard::Panic));
    assert_eq!(hear("halt"), Some(Heard::Panic));
    assert_eq!(hear("drop everything"), Some(Heard::Panic));
    assert_eq!(hear("stop"), Some(Heard::Cancel), "plain stop is still just stop");
}

#[test]
fn panicking_abandons_work_rather_than_suspending_it() {
    let mut a = Attention::default();
    a.suspend(7);
    assert_eq!(a.halt(100), "Everything stopped.");
    assert!(a.is_paused());
    assert!(a.was_halted());
    assert!(a.release().is_empty(), "nothing is waiting to resume");
}

#[test]
fn resuming_after_a_panic_does_not_quietly_restart_what_was_dropped() {
    let mut a = Attention::default();
    a.halt(100);
    let said = a.resume(200);
    assert!(said.contains("dropped what was running"), "got: {said}");
    assert!(!a.was_halted(), "and the flag clears");
}

// ================= not knowing =================

fn grounded() -> Grounding {
    Grounding { from_a_source: true, ..Default::default() }
}

#[test]
fn a_grounded_answer_is_said_as_it_is() {
    let (level, _, _) = assess("The file is 4.2 megabytes.", &grounded(), &CertaintyConfig::default());
    assert_eq!(level, Confidence::Fine);
}

#[test]
fn a_pile_of_hedging_is_a_sign_it_is_filling_a_gap() {
    let answer = "I think it's probably around there, though I'm not sure, it might be.";
    let (level, _, why) = assess(answer, &Grounding::default(), &CertaintyConfig::default());
    assert_ne!(level, Confidence::Fine);
    assert!(why.contains("hedged"), "got: {why}");
}

#[test]
fn a_claim_about_your_machine_made_without_looking_is_withheld() {
    // This is the one that matters. A model will say "your Documents folder
    // has 40 files" with no idea whether that's true.
    let answer = "Your Documents folder has the report you're after.";
    let g = Grounding { about_your_world: true, had_the_context: false, from_a_source: false };
    let (level, _, why) = assess(answer, &g, &CertaintyConfig::default());
    assert_eq!(level, Confidence::Withhold);
    assert!(why.contains("without checking"), "got: {why}");
}

#[test]
fn the_same_claim_is_fine_when_atlas_actually_looked() {
    let answer = "Your Documents folder has the report you're after.";
    let g = Grounding { about_your_world: true, had_the_context: true, from_a_source: true };
    let (level, _, _) = assess(answer, &g, &CertaintyConfig::default());
    assert_eq!(level, Confidence::Fine);
}

#[test]
fn invented_exact_figures_count_against_an_ungrounded_answer() {
    let with = assess("It's about 1234.56 per month.", &Grounding::default(), &CertaintyConfig::default());
    let without = assess("It's roughly a thousand a month.", &Grounding::default(), &CertaintyConfig::default());
    assert!(with.1 < without.1, "precision without a source is suspicious");
}

#[test]
fn not_knowing_is_said_out_loud_with_what_would_settle_it() {
    // Silence would be worse. So would a confident guess.
    let said = phrase("...", Confidence::Withhold, "I had nothing to go on");
    assert!(said.starts_with("I don't know"));
    assert!(said.contains("Ask me to look it up"), "got: {said}");
}

#[test]
fn a_middling_answer_is_given_with_the_doubt_attached() {
    let said = phrase("It's the second one.", Confidence::Qualify, "it hedged");
    assert!(said.starts_with("It's the second one."));
    assert!(said.contains("not certain"));
}

#[test]
fn the_check_can_be_switched_off() {
    let off = CertaintyConfig { enabled: false, ..Default::default() };
    let (level, _, _) = assess("total nonsense with i think and probably", &Grounding::default(), &off);
    assert_eq!(level, Confidence::Fine);
}
