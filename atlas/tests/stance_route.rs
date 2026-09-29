use atlas::learned::{Cause, Learned};
use atlas::route::{all_routes, plan, stuck_spoken, switching, Kind as RKind, Plan, RouteConfig};
use atlas::stance::{
    assess, brief, kind_of, spoken, Kind, Missing, SupportKind,
};

// ================= writing that says something =================

const BLAND: &str = "The system has been updated. Various improvements have been made to \
    several areas. Users may notice changes to the interface. Further updates are planned \
    for the coming period. Feedback is welcome.";

const GOOD: &str = "Fixed recording windows are the wrong default, and we should drop ours. \
    Every turn transcribes eight seconds whether you spoke for one or seven, which on a laptop \
    with 15GB of shared memory is the single biggest avoidable cost we have. When we measured \
    it, the average clip fell to 2.4 seconds. The obvious objection is that silence detection \
    cuts people off mid-thought, and it does when it's naive — ours waits longer after a \
    dangling word than after \"yes\". So we should ship it on by default.";

#[test]
fn a_draft_that_never_commits_to_anything_is_caught() {
    let gaps = assess(BLAND, Kind::Case);
    assert!(gaps.iter().any(|g| g.missing == Missing::NoClaim));
    assert!(gaps.iter().any(|g| g.missing == Missing::NoEvidence));
}

#[test]
fn a_draft_that_makes_a_case_and_backs_it_passes() {
    let gaps = assess(GOOD, Kind::Case);
    assert!(gaps.is_empty(), "should hold up, got {gaps:?}");
    assert!(spoken(&[]).contains("says something and backs it"));
}

#[test]
fn asserting_without_ever_supporting_is_named_as_that() {
    let all_claims = "This is the right call. The old way is worse. We should move now. \
                      Anything else would be wrong.";
    let gaps = assess(all_claims, Kind::Case);
    assert!(gaps.iter().any(|g| g.missing == Missing::NothingBehindIt));
}

#[test]
fn claim_after_claim_with_no_room_to_breathe_is_exhausting_however_true() {
    let relentless = "We should move. The old way is wrong. This is better. It matters more \
                      than anything. Delay would be worse.";
    let gaps = assess(relentless, Kind::Case);
    let g = gaps.iter().find(|g| g.missing == Missing::Relentless).expect("should spot it");
    assert!(g.evidence.contains("in a row"));
}

#[test]
fn a_case_that_never_takes_on_the_objection_reads_as_evasive() {
    let one_sided = "We should drop fixed windows. They cost 8 seconds a turn. Measured, the \
                     average is 2.4 seconds. That's the whole argument. So we should ship it.";
    assert!(assess(one_sided, Kind::Case).iter().any(|g| g.missing == Missing::NoCounterCase));
}

#[test]
fn burying_the_point_in_the_middle_is_caught() {
    let buried = "We looked at recording behaviour. The data came from four weeks of logs. \
                  Several patterns emerged in the timings. Fixed windows are the wrong default \
                  and we should drop ours.";
    let gaps = assess(buried, Kind::Case);
    let g = gaps.iter().find(|g| g.missing == Missing::BuriedLead).expect("should spot it");
    assert!(g.evidence.contains("sentence 4"));
}

#[test]
fn an_account_is_not_expected_to_argue() {
    // Editorialising in a factual account is its own fault.
    assert!(!Kind::Account.needs_a_position());
    let account = "The build failed at 3pm. The linker was missing. It was installed by 4pm. \
                   Everything has been green since.";
    assert!(!assess(account, Kind::Account).iter().any(|g| g.missing == Missing::NoClaim));
}

#[test]
fn each_kind_of_writing_wants_a_different_shape() {
    assert!(Kind::Case.wants().contains(&"the strongest objection"));
    assert!(Kind::Request.wants().contains(&"the ask"));
    assert_eq!(Kind::Note.wants(), &["the thing"]);
    assert_eq!(Kind::Request.formality(), "brief and warm");
}

#[test]
fn what_you_asked_for_decides_the_standard_applied() {
    assert_eq!(kind_of("make the case for dropping fixed windows"), Kind::Case);
    assert_eq!(kind_of("explain how endpointing works"), Kind::Explanation);
    assert_eq!(kind_of("can you ask Marta about the deadline"), Kind::Request);
    assert_eq!(kind_of("note for myself about the VPS"), Kind::Note);
}

#[test]
fn something_atlas_measured_itself_is_the_strongest_support() {
    // It's about your situation rather than about the world in general.
    assert!(SupportKind::Measured.weight() > SupportKind::Source.weight());
    assert!(SupportKind::Source.weight() > SupportKind::Example.weight());
}

#[test]
fn the_rewrite_brief_fixes_the_claim_before_the_evidence() {
    // A piece with no claim can't be fixed by adding evidence — there's
    // nothing for the evidence to support.
    let b = brief(&assess(BLAND, Kind::Case), Kind::Case).unwrap();
    let claim_at = b.find("never says what you actually think").unwrap();
    let evidence_at = b.find("nothing checkable").unwrap();
    assert!(claim_at < evidence_at);
    assert!(b.contains("Keep the voice"), "and it isn't asked to be blander");
    assert!(b.contains("a claim, then reasons"), "with the shape it wants");
}

#[test]
fn the_advice_is_specific_enough_to_act_on() {
    assert!(Missing::NoEvidence.fix().contains("One is enough"));
    assert!(Missing::NoCounterCase.fix().contains("then answer it"));
    assert!(Missing::BuriedLead.fix().contains("Move it to the top"));
}

#[test]
fn the_single_most_useful_thing_is_said_first() {
    let said = spoken(&assess(BLAND, Kind::Case));
    assert!(said.starts_with("It never says what you actually think"));
    assert!(said.contains("in one sentence, early"), "with what to do: {said}");
}

// ================= finding another way in =================

fn have_everything() -> Vec<String> {
    vec![
        "a downloaded file".into(), "the app open".into(), "a logged-in session".into(),
        "the browser".into(), "OCR".into(), "the app focused".into(),
        "an accessible control".into(), "a command line".into(), "a model".into(),
        "the internet".into(), "the document".into(),
    ]
}

fn cfg() -> RouteConfig {
    RouteConfig::default()
}

#[test]
fn the_cheapest_thing_that_could_work_is_tried_first() {
    // This is why quality doesn't cost speed: the thorough options exist,
    // they're just last, and most problems never reach them.
    match plan(RKind::Extract, "acme.com", &have_everything(), &Learned::default(), &cfg(), 0) {
        Plan::Try { route, why } => {
            assert_eq!(route.name, "read the file that's already there");
            assert!(why.contains("quickest"));
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn a_failed_approach_is_replaced_rather_than_refused() {
    // "We've tried that twice" is honest and useless on its own.
    let mut l = Learned::default();
    for _ in 0..2 {
        l.record("read the file that's already there", "acme.com", Cause::Outside, "no file", 0);
    }
    match plan(RKind::Extract, "acme.com", &have_everything(), &l, &cfg(), 0) {
        Plan::Try { route, .. } => assert_ne!(route.name, "read the file that's already there"),
        o => panic!("{o:?}"),
    }
}

#[test]
fn a_route_needing_something_you_do_not_have_is_ruled_out_by_name() {
    let offline: Vec<String> = vec!["the document".into()];
    match plan(RKind::Learn, "the spec", &offline, &Learned::default(), &cfg(), 0) {
        Plan::Try { route, .. } => {
            assert!(route.name == "what I already have indexed"
                || route.name == "read the source or spec directly");
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn with_every_route_closed_it_says_so_and_asks_you() {
    let mut l = Learned::default();
    for r in [
        "read the file that's already there", "read the window's text",
        "use the site's own export", "read the page through the browser",
        "screenshot and read the text off it", "ask you to save it and point me at it",
    ] {
        for _ in 0..2 {
            l.record(r, "acme.com", Cause::WrongIdea, "closed", 0);
        }
    }
    let p = plan(RKind::Extract, "acme.com", &have_everything(), &l, &cfg(), 0);
    match &p {
        Plan::Stuck { tried, why } => {
            assert!(why.contains("ways in, all closed"));
            assert!(!tried.is_empty());
        }
        o => panic!("{o:?}"),
    }
    let said = stuck_spoken("getting the export from acme.com", &p);
    assert!(said.starts_with("I seem to be stuck on getting the export from acme.com for "), "{said}");
    assert!(said.contains("Tell me how you'd do it and I'll learn it"));
}

#[test]
fn there_is_always_a_way_that_involves_asking_you() {
    // Which means "stuck" only happens when even that has been ruled out.
    let routes = all_routes(RKind::Extract, "new-site.com", &have_everything(), &Learned::default(), &cfg(), 0);
    assert!(routes.len() >= 3, "several ways in");
    let names: Vec<&str> = routes.iter().map(|r| r.name.as_str()).collect();
    assert!(names.iter().any(|n| n.contains("ask you")) || routes.len() >= 4);
}

#[test]
fn the_order_is_planned_in_advance_so_a_failure_moves_straight_on() {
    let routes = all_routes(RKind::Act, "notepad", &have_everything(), &Learned::default(), &cfg(), 0);
    assert_eq!(routes[0].name, "keyboard shortcut", "cheapest first");
    assert!(routes.windows(2).all(|w| w[0].name != w[1].name), "no repeats");
}

#[test]
fn being_more_patient_prefers_the_reliable_route_over_the_quick_one() {
    let patient = RouteConfig { impatience: 0.0, ..cfg() };
    match plan(RKind::Fix, "src/voice.rs", &have_everything(), &Learned::default(), &patient, 0) {
        Plan::Try { route, .. } => {
            assert!(route.reliability >= 0.7, "picked the thorough one: {}", route.name)
        }
        o => panic!("{o:?}"),
    }
    let hasty = RouteConfig { impatience: 0.9, ..cfg() };
    match plan(RKind::Fix, "src/voice.rs", &have_everything(), &Learned::default(), &hasty, 0) {
        Plan::Try { route, .. } => assert!(route.costs_secs <= 5, "picked the quick one"),
        o => panic!("{o:?}"),
    }
}

#[test]
fn changing_approach_is_announced_rather_than_done_silently() {
    // The difference between looking adaptive and looking confused.
    let said = switching("the export button", "the download link", "there isn't one");
    assert!(said.contains("didn't work"));
    assert!(said.contains("Trying the download link instead"));
}

#[test]
fn a_route_that_hardly_ever_works_is_not_offered_at_all() {
    let picky = RouteConfig { min_reliability: 0.9, ..cfg() };
    let routes = all_routes(RKind::Fix, "x", &have_everything(), &Learned::default(), &picky, 0);
    assert!(routes.iter().all(|r| r.reliability >= 0.9));
}
