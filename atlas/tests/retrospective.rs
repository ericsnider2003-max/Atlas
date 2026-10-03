//! The pipeline's questions, asked of everything built before it existed.
//!
//! Applying thought-build-review-refine to 145 modules by hand isn't work
//! anyone should do — it's work that should be *found*. These checks look for
//! the two failures the pipeline exists to catch, across the whole suite:
//!
//! 1. A test that would pass whether or not the code did anything.
//! 2. A module whose stated purpose and actual behaviour have drifted apart.
//!
//! Each is a ratchet with a baseline. The number only goes down.

use std::collections::BTreeMap;
use std::fs;

fn test_files() -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if let Ok(dir) = fs::read_dir("tests") {
        for e in dir.flatten() {
            let p = e.path();
            // This file contains the word it searches for, in string
            // literals. Scanning itself finds its own parser.
            if p.file_stem().map(|n| n == "retrospective").unwrap_or(false) {
                continue;
            }
            if p.extension().map(|x| x == "rs").unwrap_or(false) {
                if let Ok(text) = fs::read_to_string(&p) {
                    out.insert(p.file_stem().unwrap().to_string_lossy().to_string(), text);
                }
            }
        }
    }
    out
}

/// Split a file into its test functions.
fn tests_in(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("#[test]") {
        rest = &rest[i + 7..];
        let Some(name_at) = rest.find("fn ") else { break };
        let after = &rest[name_at + 3..];
        let Some(paren) = after.find('(') else { break };
        let name = after[..paren].trim().to_string();
        // The body runs to the next test, or the end.
        let body_end = after.find("#[test]").unwrap_or(after.len());
        out.push((name, after[..body_end].to_string()));
    }
    out
}

/// Tests that assert nothing at all.
///
/// A test with no assertion passes for as long as the code compiles, which is
/// not what anyone reading the count believes it means.
#[test]
fn no_test_asserts_nothing() {
    let mut silent = Vec::new();
    for (file, text) in test_files() {
        for (name, body) in tests_in(&text) {
            let asserts = body.contains("assert")
                || body.contains("panic!")
                || body.contains(".unwrap()")
                || body.contains(".expect(");
            if !asserts {
                silent.push(format!("{file}::{name}"));
            }
        }
    }
    assert!(silent.is_empty(), "tests that assert nothing: {silent:#?}");
}

/// Tests whose only assertions are about a constant string.
///
/// These are the ones I wrote most of, and they're worth being honest about:
/// asserting that a message contains a phrase proves the phrase is there, not
/// that anything behaves correctly. They're documentation with a test harness
/// around them.
///
/// Useful, but they should never be the *only* test of a behaviour — so this
/// counts them and holds the line rather than banning them.
///
/// Raised from 37 to 41 on 11 Sep 2026, the first day this guard could run at
/// all: it needs `cargo test`, which needed crates.io, which had no path to
/// this tree before that day. The true count was 41 the moment it could first
/// be measured — this is old debt made visible, not four new prose-only tests
/// written that day. Named rather than fixed for the same reason
/// `DEAD_CAPABILITY_CEILING` was raised alongside it: turning a documentation
/// test into a real behavioural one means understanding what it was meant to
/// prove, module by module, not guessing four times in one pass. The ratchet
/// resumes from here: it may only shrink.
/// Tests that assert a wording rather than a behaviour.
///
/// Named so that clearing one is visible. A count could not tell "gave two
/// real assertions, wrote two more prose-only ones" from "did nothing".
///
/// **This was `usize = 41` on this side until the second 17 Sep merge.** The
/// improvements chat made it a named list and that is strictly better, for
/// the same reason `bug_sweep`'s stale-doc count became one: a number can be
/// satisfied by editing the number. Their list is taken as the mechanism;
/// the entries are measured on THIS tree, which is why
/// `helpers_are_governed::a_refusal_is_a_sentence_a_person_can_read` is not
/// among them -- it was on their list, and earlier today it gained the
/// assertion it was missing (that a refused helper does not still start the
/// thing it refused), so it is not prose-only here any more. The second
/// assertion below is what would have caught that drift.
const PROSE_ONLY_BASELINE: &[&str] = &[
    // 2 Oct 2026, merge of Chat A's learning-how-you-talk: both drive a real
    // Daemon turn and read the prompt the stand-in model was actually sent
    // (people, project, a learned wording as a hint); the shape heuristic
    // reads `ProactiveConfig::default()` as a constant.
    "learning_how_you_talk::a_learned_wording_is_a_hint_when_it_is_only_close",
    "learning_how_you_talk::the_people_and_project_a_sentence_names_reach_the_model",
    // 2 Oct 2026, merge of Chat A's PC organizing: drives a real Daemon
    // turn with system changes off and checks the refusal it gives back; the
    // shape heuristic reads `ProactiveConfig::default()` as a constant.
    // 3 Oct 2026 (integration-1003): chat-c-3 baselined the four
    // `atlas_writes_real_code` names as prose-only, but on the merged tree they
    // drive a real Daemon turn and check something real, so they are NOT
    // baselined here -- the guard below would (correctly) reject them.
    "sorting_a_folder::with_system_changes_off_it_says_so_and_moves_nothing",
    // 28 Sep 2026, round 3 merge: both drive a real Daemon through
    // hublive::reply (a failed save; Pause) and check what comes back; the
    // shape heuristic reads the constants they also check as the whole test.
    "hublive::a_file_from_the_phone_that_could_not_be_kept_says_so",
    "the_microphone_has_its_own_thread::the_icon_and_the_hub_say_pause_turns_the_microphone_off",
    // 28 Sep 2026, merge: a source guard (what starts Atlas on Windows never
    // goes through a console) and a rendered page checked for command text;
    // the shape heuristic reads the enum paths as constants.
    "atlas_runs_with_nothing_open::every_way_atlas_starts_goes_to_atlas_exe_and_never_a_console",
    "the_hub_holds_the_way_back::nothing_the_hub_says_about_these_names_a_command",
    // 28 Sep 2026: three flagged by the shape heuristic but behavioural --
    // each drives the daemon (`execute`) or the hub and checks what came
    // back and what the stand-in program was asked; the capital-letter
    // `::` lines are enum variants, not constants.
    "mail_through_himalaya::a_mail_check_goes_through_himalaya_with_nothing_from_the_vault",
    "other_programs_tools::a_tool_whose_program_has_gone_is_said_plainly_through_the_daemon",
    "reading_without_the_picture_reader::a_handed_photo_is_read_by_the_recognizer_when_atlas_has_no_reading_models",
    "a_brief_without_an_inbox::most_blocking_finally_has_something_to_work_on",
    // 29 Sep 2026, round 6 merge: behavioural, misread by the shape
    // heuristic. The five social ones build a record book from made-up posts
    // and account snapshots and check the numbers the analysis worked out of
    // it (medians, week and month deltas, per-slot bests, escaping on the
    // rendered page); the wit one asks the persona for a turn on money, a
    // rough patch and a chat and checks what it tells the model, then scans
    // the drafting modules. The capital-letter `::` lines are enum variants
    // (`Platform::Youtube`, `Record::Account`, `Register::Rough`), not
    // constants. (`photo_editing::real_photos_are_measured_for_tilt_and_say_how_sure`
    // was flagged too, and really did only print what it said: it now checks
    // the sentence against what was measured, so it is not listed.)
    "social::under_the_pattern_floor_it_gives_numbers_and_calls_patterns_noise",
    "social::over_the_floor_it_names_the_opening_length_and_retention_that_did_best",
    "social::posting_times_need_enough_posts_in_each_slot",
    "social::followers_over_time_per_platform_and_together_naming_the_gaps",
    "social::the_social_page_escapes_what_it_shows_and_says_when_the_vault_is_locked",
    "wit_setting::the_model_is_told_no_jokes_when_it_matters_and_never_asked_for_one_in_a_draft",
    // 22 Sep 2026: afterme_companion::an_arrangement_someone_has_not_been_told_about_is_not_one now asserts telling the person empties the gap list.
    // 22 Sep 2026: afterme_companion::an_envelope_nobody_can_find_is_the_same_as_no_envelope now asserts the gap is ranked top-tier (urgency >= 0.9).
    // 22 Sep 2026: afterme_companion::no_envelope_at_all_is_the_top_of_the_list now asserts the gap's urgency is the maximum 1.0.
    // 22 Sep 2026: audio_enumeration::a_headerless_line_is_not_mistaken_for_a_device now asserts exactly three real devices parse.
    // 22 Sep 2026: away_voiceover::accounts_that_would_all_survive_get_nothing now asserts would_lock_you_out is empty.
    "confirmed::generating_new_codes_says_the_old_ones_stop_working",
    "confirmed::how_to_undo_it_is_answered_because_you_will_ask_and_will_not_remember",
    "confirmed::the_read_back_carries_the_consequence_you_might_not_have_in_mind",
    // 22 Sep 2026: daily_booking::what_you_hear_when_the_day_turns_is_short now asserts the live task is carried, not finished.
    // 22 Sep 2026: editcraft::perpetual_rights_are_countered_with_a_term_rather_than_refused now asserts the ask returns a counter (Some) while a no-counter case is None.
    // 22 Sep 2026: enrol::a_refusal_says_which_button_it_refused now also asserts the named button is actually blocked.
    "enrol::the_consent_line_says_what_you_are_agreeing_to",
    // 22 Sep 2026: files_remote::a_failure_is_worth_hearing_however_quick_it_was now asserts a quick failure still surfaces (is_some).
    "files_remote::finishing_says_how_long_it_took_in_units_you_would_use",
    // 22 Sep 2026: finance_consent::the_summary_leads_with_how_much_there_is_to_hear now asserts review is non-empty behind the count.
    // 22 Sep 2026: flight_recorder::asked_what_it_has_been_doing_it_says_so_out_loud now asserts the trace actually recorded the call.
    "hub_settings::settings_mode_starts_nothing_that_can_be_broken",
    "install::nothing_needs_a_key_an_account_or_a_card",
    // 22 Sep 2026: interrupt_mail::each_provider_says_what_you_actually_have_to_do now asserts two providers give different instructions.
    // 22 Sep 2026: money_grading::things_it_could_not_place_are_admitted_rather_than_guessed_into_a_bucket now asserts sort_one leaves an opaque line as Bucket::Unknown.
    "no_quiet_nothings::the_audit_asks_only_questions_that_change_nothing",
    "one_install_root::what_atlas_promises_to_keep_includes_the_trash",
    // 22 Sep 2026: opsec_recovery_undo::everything_in_order_says_so_briefly now asserts the gap list is empty when it says "all checked".
    // 22 Sep 2026: opsec_recovery_undo::naming_an_area_undoes_the_right_thing_rather_than_the_last_thing now asserts the files entry differs from the last entry.
    // 22 Sep 2026: opsec_recovery_undo::something_only_you_can_undo_says_where now asserts the reversal is Reversal::OverToYou, not CanDo.
    // 22 Sep 2026: opsec_recovery_undo::two_routes_that_are_both_one_person_is_worth_mentioning now asserts a split route present removes the gap.
    "panels::the_palette_avoids_both_obvious_routes",
    // 22 Sep 2026: person::everything_it_thinks_it_knows_can_be_read now asserts show() reflects the real stored trait count.
    "phase2::with_no_microphone_it_says_we_will_type_rather_than_failing",
    // 22 Sep 2026: phase3::an_approach_that_was_never_going_to_work_is_refused_the_first_time now asserts a one-off world failure gets TryAgain instead.
    // 22 Sep 2026: plainly_publishing::a_well_protected_set_of_accounts_is_left_alone now asserts audit() produced no advice.
    // 22 Sep 2026: plainly_publishing::atlas_says_when_a_piece_is_the_wrong_length_for_where_it_is_going now asserts a well-fitted piece draws less advisory text.
    "returning_otherside::what_it_rests_on_that_you_do_not_control_is_named",
    "signin::what_atlas_will_and_will_not_do_on_a_bank_is_said_in_one_place",
    // 22 Sep 2026: strategy::the_same_error_three_times_running_stops_it_early now asserts next() returns Next::Exhausted.
    // 22 Sep 2026: subsystems::an_unknown_variable_is_left_visible_rather_than_blanked now asserts the string is passed through unchanged.
    "sync::a_bundle_is_not_a_file_you_have_to_manage",
];

#[test]
fn the_number_of_documentation_shaped_tests_does_not_grow() {
    let mut prose_only = Vec::new();
    for (file, text) in test_files() {
        for (name, body) in tests_in(&text) {
            let lines: Vec<&str> = body.lines().filter(|l| l.contains("assert")).collect();
            if lines.is_empty() {
                continue;
            }
            // Every assertion is `something.contains("...")` on a constant.
            let all_contains = lines.iter().all(|l| l.contains(".contains("));
            let touches_a_const = body
                .lines()
                .any(|l| l.contains("::") && l.chars().filter(|c| c.is_uppercase()).count() > 4);
            if all_contains && touches_a_const {
                prose_only.push(format!("{file}::{name}"));
            }
        }
    }
    // Named, not counted -- `prose_only` is built one line above and was
    // discarded for its length. A pile of tests that prove a wording rather
    // than a behaviour, with no record of which ones, is exactly the shape
    // this file exists to criticise. The list stood at 41; on 22 Sep 2026 four
    // passes (six, eight, seven, then five) gave real behavioural assertions
    // and removed them, leaving 15.
    let unexplained: Vec<&String> =
        prose_only.iter().filter(|t| !PROSE_ONLY_BASELINE.contains(&t.as_str())).collect();
    assert!(
        unexplained.is_empty(),
        "these assert only that a constant contains a phrase, which proves the \
         wording and not the behaviour:\n  {}\n\nGive them something real to \
         check, or add them to PROSE_ONLY_BASELINE.",
        unexplained.iter().map(|t| t.as_str()).collect::<Vec<_>>().join("\n  ")
    );
    let improved: Vec<&&str> =
        PROSE_ONLY_BASELINE.iter().filter(|b| !prose_only.iter().any(|t| t == *b)).collect();
    assert!(
        improved.is_empty(),
        "these are baselined as prose-only and now check something real. Remove \
         them from the baseline so the list keeps meaning what it says:\n  \
         {improved:?}"
    );
}

/// Every module with behaviour has at least one test that exercises it.
///
/// Not "has a test file" — has a test that calls into it.
#[test]
fn every_module_with_behaviour_is_exercised_somewhere() {
    let all_tests: String = test_files().values().cloned().collect::<Vec<_>>().join("\n");
    let mut untested = Vec::new();

    for e in fs::read_dir("src").unwrap().flatten() {
        let p = e.path();
        if p.extension().map(|x| x != "rs").unwrap_or(true) {
            continue;
        }
        let name = p.file_stem().unwrap().to_string_lossy().to_string();
        if ["main", "lib"].contains(&name.as_str()) {
            continue;
        }
        // The module with any `src/<name>/` children it is split into (27 Sep 2026).
        let src = crate::common::source_of(&name);
        // Modules that are only types and constants have nothing to exercise.
        let has_behaviour = src.matches("pub fn ").count() >= 2;
        if !has_behaviour {
            continue;
        }
        let called = all_tests.contains(&format!("atlas::{name}::"))
            || all_tests.contains(&format!("use atlas::{name}"))
            || src.contains("#[cfg(test)]");
        if !called {
            untested.push(name);
        }
    }
    assert!(untested.is_empty(), "modules with behaviour and no test that calls them: {untested:#?}");
}

/// A module's opening comment should describe what it does.
///
/// Cheap check for the drift the review stage catches by hand: a file whose
/// documentation stops matching its contents.
#[test]
fn every_module_says_what_it_is_for() {
    let mut undocumented = Vec::new();
    for e in fs::read_dir("src").unwrap().flatten() {
        let p = e.path();
        if p.extension().map(|x| x != "rs").unwrap_or(true) {
            continue;
        }
        let name = p.file_stem().unwrap().to_string_lossy().to_string();
        if ["main", "lib"].contains(&name.as_str()) {
            continue;
        }
        let src = fs::read_to_string(&p).unwrap_or_default();
        let doc: String = src.lines().take_while(|l| l.starts_with("//!")).collect();
        // A one-line comment is a label, not a description of why it exists.
        if doc.len() < 80 {
            undocumented.push(name);
        }
    }
    assert!(undocumented.is_empty(), "modules with no real explanation: {undocumented:#?}");
}

/// Anything that claims to be non-configurable actually is.
///
/// Across the whole codebase: a `#[serde(skip)]` field with a fixed default is
/// the pattern used for "this can't be turned off". If one of those ever
/// stopped being skipped, the guarantee would quietly disappear.
#[test]
fn nothing_claiming_to_be_fixed_can_be_set_from_config() {
    let mut leaky = Vec::new();
    for e in fs::read_dir("src").unwrap().flatten() {
        let p = e.path();
        if p.extension().map(|x| x != "rs").unwrap_or(true) {
            continue;
        }
        let name = p.file_stem().unwrap().to_string_lossy().to_string();
        // The module with any `src/<name>/` children it is split into (27 Sep 2026).
        let src = crate::common::source_of(&name);

        for (i, line) in src.lines().enumerate() {
            let says_fixed = line.contains("Not configurable")
                || line.contains("not configurable")
                || line.contains("isn't configurable");
            if !says_fixed {
                continue;
            }
            // The field it's describing should be within the next few lines
            // and should be skipped.
            let after: String = src.lines().skip(i).take(6).collect::<Vec<_>>().join("\n");
            if after.contains("pub ") && !after.contains("serde(skip") {
                leaky.push(format!("{name}:{}", i + 1));
            }
        }
    }
    assert!(
        leaky.is_empty(),
        "claims to be non-configurable but the field is readable from YAML: {leaky:#?}"
    );
}
