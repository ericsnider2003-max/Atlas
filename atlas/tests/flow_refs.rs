use atlas::categories::{category_of, consent_line, media_category, media_decision, Category, MediaOp};
use atlas::config::Config;
use atlas::flow::{expand, Library, Next, OnFail, Run, RunState, Step, Workflow};
use atlas::intent::Intent;
use atlas::policy::Decision;
use atlas::references::{has_pronoun, resolve, Referents, Resolution};
use std::collections::BTreeMap;
use std::path::Path;

// ================= follow-ups =================

fn refs() -> Referents {
    Referents { last_app: Some("chrome".into()), ..Default::default() }
}

#[test]
fn text_that_names_its_target_is_left_alone() {
    assert_eq!(resolve("close chrome", &refs()), Resolution::Unchanged("close chrome".into()));
}

#[test]
fn a_pronoun_resolves_to_what_atlas_just_acted_on() {
    match resolve("close it", &refs()) {
        Resolution::Resolved { text, referent } => {
            assert_eq!(text, "close chrome");
            assert_eq!(referent, "chrome");
        }
        other => panic!("expected resolution, got {other:?}"),
    }
}

#[test]
fn a_dangling_pronoun_becomes_a_question_never_a_guess() {
    // Guessing here means acting on the wrong window.
    let r = resolve("close it", &Referents::default());
    assert!(matches!(r, Resolution::Ambiguous(_)));
    assert_eq!(r.text(), "Which app?");
}

#[test]
fn the_question_matches_what_kind_of_thing_was_meant() {
    let empty = Referents::default();
    assert_eq!(resolve("close that", &empty).text(), "Which app?");
    assert_eq!(resolve("open that", &empty).text(), "Which file?");
    assert_eq!(resolve("research that", &empty).text(), "About what?");
}

#[test]
fn the_verb_decides_which_kind_of_referent_is_preferred() {
    let r = Referents {
        last_app: Some("chrome".into()),
        last_note: Some("research-note.md".into()),
        ..Default::default()
    };
    match resolve("close it", &r) {
        Resolution::Resolved { referent, .. } => assert_eq!(referent, "chrome"),
        o => panic!("{o:?}"),
    }
    match resolve("open it", &r) {
        Resolution::Resolved { referent, .. } => assert_eq!(referent, "research-note.md"),
        o => panic!("{o:?}"),
    }
}

#[test]
fn only_the_first_pronoun_is_replaced() {
    let r = resolve("move it and then close it", &refs());
    assert_eq!(r.text(), "move chrome and then close it");
}

#[test]
fn there_is_a_destination_not_a_thing_so_it_is_left_alone() {
    // "move it there" — Atlas cannot infer which screen you pointed at.
    let r = resolve("move it there", &refs());
    assert_eq!(r.text(), "move chrome there");
}

#[test]
fn punctuation_and_case_do_not_defeat_pronoun_detection() {
    assert!(has_pronoun("Close It."));
    assert!(has_pronoun("what about THAT?"));
    assert!(!has_pronoun("close chrome"));
}

#[test]
fn the_active_window_is_a_fallback_when_atlas_has_not_acted_yet() {
    let r = Referents { active_app: Some("notepad".into()), ..Default::default() };
    match resolve("close it", &r) {
        Resolution::Resolved { referent, .. } => assert_eq!(referent, "notepad"),
        o => panic!("{o:?}"),
    }
}

// ================= the four categories =================

#[test]
fn ordinary_workspace_work_is_local() {
    assert_eq!(category_of(&Intent::OpenApp("chrome".into())), Category::LocalOperational);
    assert_eq!(category_of(&Intent::WorkspaceOn), Category::LocalOperational);
}

#[test]
fn research_leaves_the_machine_but_does_not_upload_your_content() {
    let c = category_of(&Intent::Research("x".into()));
    assert_eq!(c, Category::StandardExternal);
}

#[test]
fn where_media_work_runs_decides_its_category_not_what_it_does() {
    assert_eq!(media_category(MediaOp::Edit, false), Category::LocalCreative);
    assert_eq!(media_category(MediaOp::Edit, true), Category::ExternalAiCreative);
}

#[test]
fn sending_your_content_to_an_outside_ai_always_needs_approval() {
    assert_eq!(media_decision(MediaOp::Generate, true, false), Decision::RequireApproval);
}

#[test]
fn local_media_work_is_done_and_reported_not_gated() {
    assert_eq!(media_decision(MediaOp::Edit, false, false), Decision::ProceedAndReport);
}

#[test]
fn overwriting_an_original_needs_approval_wherever_it_ran() {
    // That is about destroying a file, not about where the compute happened.
    assert_eq!(media_decision(MediaOp::Edit, false, true), Decision::RequireApproval);
    assert_eq!(media_decision(MediaOp::Export, false, false), Decision::RequireApproval);
}

#[test]
fn the_consent_line_says_what_you_are_actually_agreeing_to() {
    let line = consent_line(Category::ExternalAiCreative, "Upscaling that photo");
    assert!(line.contains("outside AI service"), "got: {line}");
    assert!(line.ends_with("Go ahead?"));
}

// ================= multi-step work =================

fn research_flow() -> Workflow {
    Workflow {
        name: "morning brief".into(),
        triggers: vec!["morning brief".into()],
        steps: vec![
            Step::new("research overnight market news").producing("brief"),
            Step::new("save note {brief}"),
            Step::new("open notepad").optional(),
        ],
    }
}

#[test]
fn a_chain_runs_its_steps_in_order() {
    let mut r = Run::start(&research_flow());
    assert_eq!(r.next(), Next::Run("research overnight market news".into()));
    r.report("note-1.md", true);
    assert_eq!(r.next(), Next::Run("save note note-1.md".into()));
    r.report("saved", true);
    assert_eq!(r.next(), Next::Run("open notepad".into()));
    r.report("ok", true);
    assert_eq!(r.next(), Next::Finished);
    assert_eq!(r.state, RunState::Done);
}

#[test]
fn one_steps_output_is_available_to_the_next() {
    // This is the content-staging piece — step 2 uses what step 1 produced.
    let mut r = Run::start(&research_flow());
    r.report("data/notes/brief.md", true);
    assert_eq!(r.next(), Next::Run("save note data/notes/brief.md".into()));
}

#[test]
fn a_failure_stops_the_chain_by_default() {
    // Later steps usually assume earlier ones worked.
    let mut r = Run::start(&research_flow());
    r.report("no connection", false);
    assert_eq!(r.state, RunState::Failed);
    assert!(matches!(r.next(), Next::Stopped(_)));
}

#[test]
fn an_optional_step_failing_does_not_kill_the_chain() {
    let mut r = Run::start(&research_flow());
    r.report("note.md", true);
    r.report("saved", true);
    r.report("notepad missing", false); // marked optional
    assert_eq!(r.state, RunState::Done);
}

#[test]
fn a_retrying_step_is_tried_the_configured_number_of_times() {
    let w = Workflow {
        name: "flaky".into(),
        triggers: vec![],
        steps: vec![Step::new("fetch something").retrying(2)],
    };
    let mut r = Run::start(&w);
    r.report("timeout", false);
    assert_eq!(r.state, RunState::Running, "attempt 1 failed, try again");
    r.report("timeout", false);
    assert_eq!(r.state, RunState::Running, "attempt 2 failed, one left");
    r.report("timeout", false);
    assert_eq!(r.state, RunState::Failed, "out of retries");
}

#[test]
fn a_retry_that_succeeds_moves_on_and_resets_the_counter() {
    let w = Workflow {
        name: "flaky".into(),
        triggers: vec![],
        steps: vec![Step::new("a").retrying(3), Step::new("b").retrying(3)],
    };
    let mut r = Run::start(&w);
    r.report("timeout", false);
    r.report("ok", true);
    assert_eq!(r.next(), Next::Run("b".into()));
    r.report("timeout", false);
    assert_eq!(r.state, RunState::Running, "counter reset for the new step");
}

#[test]
fn a_chain_can_pause_for_approval_without_losing_its_place() {
    let mut r = Run::start(&research_flow());
    r.report("note.md", true);
    r.needs_approval();
    assert_eq!(r.next(), Next::Approve("save note note.md".into()));
    r.approve();
    assert_eq!(r.next(), Next::Run("save note note.md".into()), "resumes exactly where it was");
}

#[test]
fn refusing_mid_chain_abandons_the_rest_rather_than_half_doing_it() {
    let mut r = Run::start(&research_flow());
    r.report("note.md", true);
    r.needs_approval();
    r.deny();
    assert_eq!(r.state, RunState::Failed);
    assert!(r.log.last().unwrap().contains("declined"));
}

#[test]
fn progress_is_reportable_in_one_spoken_line() {
    let mut r = Run::start(&research_flow());
    assert_eq!(r.summary(), "morning brief: step 1 of 3.");
    r.report("x", true);
    r.report("x", true);
    r.report("x", true);
    assert!(r.summary().contains("all 3 steps done"));
}

#[test]
fn unknown_placeholders_are_left_visible_rather_than_blanked() {
    let mut out = BTreeMap::new();
    out.insert("known".to_string(), "value".to_string());
    assert_eq!(expand("{known} and {typo}", &out), "value and {typo}");
}

#[test]
fn a_sequence_you_performed_becomes_a_named_chain() {
    let mut lib = Library::default();
    let w = lib.record(
        "start work",
        &["boot workspace".into(), "open chrome".into()],
        Some("start work"),
    );
    assert_eq!(w.steps.len(), 2);
    assert_eq!(lib.get("Start Work").unwrap().name, "start work");
}

#[test]
fn a_specific_trigger_is_not_shadowed_by_a_general_one() {
    let mut lib = Library::default();
    lib.add(Workflow { name: "general".into(), triggers: vec!["brief".into()], steps: vec![] });
    lib.add(Workflow {
        name: "specific".into(),
        triggers: vec!["morning brief".into()],
        steps: vec![],
    });
    assert_eq!(lib.match_trigger("give me the morning brief").unwrap().name, "specific");
}

#[test]
fn re_recording_a_chain_replaces_it_instead_of_duplicating() {
    let mut lib = Library::default();
    lib.record("start work", &["a".into()], None);
    lib.record("start work", &["a".into(), "b".into()], None);
    assert_eq!(lib.workflows.len(), 1);
    assert_eq!(lib.get("start work").unwrap().steps.len(), 2);
}

#[test]
fn chains_survive_a_restart() {
    let d = std::env::temp_dir().join("atlas-flow-test");
    let _ = std::fs::remove_dir_all(&d);
    let store = atlas::store::Store::new(&d);
    let mut lib = Library::default();
    lib.record("start work", &["boot workspace".into()], Some("start work"));
    lib.save(&store).unwrap();
    assert!(Library::load(&store).get("start work").is_some());
}

#[test]
fn the_default_failure_mode_is_stop_not_continue() {
    assert_eq!(OnFail::default(), OnFail::Stop);
}

// ================= apps Atlas must not type into =================

#[test]
fn discord_can_be_arranged_but_never_typed_into() {
    // A stray synthetic Enter in a chat client can join a call or send a
    // message. No amount of careful targeting makes that worth risking.
    let c = Config::load(Path::new("config")).unwrap();
    assert!(atlas::workspace::input_blocked_apps(&c).contains(&"discord".to_string()));
    assert!(!atlas::workspace::input_blocked_apps(&c).contains(&"notepad".to_string()));
}
