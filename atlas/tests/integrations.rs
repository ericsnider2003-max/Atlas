//! Is each connection still working?

use atlas::integrations::*;

const HOUR: u64 = 3600;
const DAY: u64 = 24 * HOUR;

fn board(now: u64) -> Board {
    let mut b = Board::default();
    let mut s = Integration::new("stripe", "Payment notifications stop.");
    s.worked(now);
    b.add(s);
    let mut m = Integration::new("mail", "I can't read or send anything.");
    m.worked(now);
    b.add(m);
    b.swept(now);
    b
}

// --- last success, not last attempt -----------------------------------------

#[test]
fn a_failure_since_the_last_success_is_the_current_state() {
    // Forty failures in a row is not "recently active".
    let mut b = board(0);
    b.get_mut("stripe").unwrap().failed(HOUR, "401 from the API");
    assert_eq!(b.get_mut("stripe").unwrap().health(HOUR), Health::Failing);
}

#[test]
fn working_again_clears_the_run_of_failures() {
    let mut b = board(0);
    let s = b.get_mut("stripe").unwrap();
    s.failed(HOUR, "timeout");
    s.failed(2 * HOUR, "timeout");
    assert_eq!(s.failures_running, 2);
    s.worked(3 * HOUR);
    assert_eq!(s.failures_running, 0);
    assert_eq!(s.health(3 * HOUR), Health::Working);
}

// --- silence is not health --------------------------------------------------

#[test]
fn an_integration_nobody_has_used_is_unknown_not_fine() {
    // Reporting it as working is the mistake this exists to stop: absence of
    // a finding read as absence of a problem.
    let b = {
        let mut b = Board::default();
        b.add(Integration::new("calendar", "I can't see what's coming."));
        b.swept(0);
        b
    };
    assert_eq!(b.integrations[0].health(0), Health::Unknown);
    assert!(b.integrations[0].health(0).wants_attention());
}

#[test]
fn one_that_worked_but_not_lately_is_quiet_rather_than_working() {
    let b = board(0);
    assert_eq!(b.integrations[0].health(DAY), Health::Working);
    assert_eq!(b.integrations[0].health(10 * DAY), Health::Quiet);
}

#[test]
fn quiet_does_not_nag() {
    // Nagging about every integration you haven't used this week is how a
    // status page stops being read.
    assert!(!Health::Quiet.wants_attention());
    assert!(Health::Failing.wants_attention());
    assert!(Health::Unknown.wants_attention());
    assert!(!Health::Off.wants_attention());
}

#[test]
fn switching_one_off_is_not_a_fault() {
    let mut b = board(0);
    b.get_mut("mail").unwrap().enabled = false;
    assert_eq!(b.get_mut("mail").unwrap().health(0), Health::Off);
    assert!(b.needs_you(0).is_empty());
}

// --- a stale monitor says so ------------------------------------------------

#[test]
fn a_monitor_that_has_not_looked_recently_says_so_first() {
    // Otherwise it reports yesterday's answer in the present tense — the exact
    // failure it was built to prevent, committed by the monitor.
    let b = board(0);
    assert!(b.is_current(HOUR));
    assert!(!b.is_current(3 * DAY));
    assert!(b.panel(3 * DAY).starts_with("Last checked"));
    assert!(b.spoken(3 * DAY).contains("haven't checked"));
}

#[test]
fn a_monitor_that_has_never_looked_is_not_current() {
    let mut b = Board::default();
    b.add(Integration::new("x", "something stops."));
    assert!(!b.is_current(0));
    assert!(b.panel(0).contains("haven't checked"));
}

// --- what the hub shows -----------------------------------------------------

#[test]
fn the_panel_leads_with_what_is_broken() {
    // A page that opens with nine greens and one red trains you to skim past
    // the red.
    let mut b = board(0);
    b.get_mut("stripe").unwrap().failed(HOUR, "401 from the API");
    b.swept(HOUR);
    let p = b.panel(HOUR);
    assert!(p.starts_with("stripe"), "got: {p}");
    assert!(p.contains("1 other fine"));
}

#[test]
fn a_failure_reads_as_a_consequence_not_a_name() {
    let mut b = board(0);
    b.get_mut("mail").unwrap().failed(HOUR, "token expired");
    b.swept(HOUR);
    let line = b.get_mut("mail").unwrap().line(HOUR);
    assert!(line.contains("token expired"));
    assert!(line.contains("can't read or send"), "no consequence stated: {line}");
}

#[test]
fn everything_healthy_says_so_shortly() {
    let b = board(0);
    assert!(b.panel(HOUR).contains("2 others fine"));
    assert_eq!(b.spoken(HOUR), "Everything's connected.");
}

#[test]
fn nothing_connected_says_that_rather_than_looking_healthy() {
    let b = Board::default();
    assert_eq!(b.panel(0), "Nothing connected yet.");
}

#[test]
fn one_problem_is_named_and_several_are_counted() {
    let mut b = board(0);
    b.get_mut("stripe").unwrap().failed(HOUR, "401");
    b.swept(HOUR);
    assert!(b.spoken(HOUR).starts_with("stripe needs a look"));
    b.get_mut("mail").unwrap().failed(HOUR, "token");
    b.swept(HOUR);
    assert!(b.spoken(HOUR).starts_with("2 connections need a look"));
}

#[test]
fn adding_the_same_name_twice_replaces_rather_than_duplicates() {
    let mut b = board(0);
    b.add(Integration::new("stripe", "changed description"));
    assert_eq!(b.integrations.len(), 2);
}

// ---------------------------------------------------------------------------
// mark() — putting the gap in the answer itself.
//
// These exist because `links.rs` was retired into this module and its five
// tests for `mark` went with it. The function survived the merge; its coverage
// did not, and nothing noticed, because `mark` was also never called from
// anywhere. It was public, untested and unreachable at the same time — three
// separate signals all pointing at a feature that read as delivered and did
// nothing.
// ---------------------------------------------------------------------------

#[test]
fn an_answer_that_needed_a_broken_source_says_so_in_the_answer() {
    let mut b = board(HOUR);
    b.get_mut("stripe").unwrap().failed(HOUR, "401");
    let out = mark("You took $40 yesterday.", &["stripe"], &b, HOUR);
    assert!(out.starts_with("You took $40 yesterday."), "the answer survives: {out}");
    assert!(out.contains("stripe is failing"), "names the source and its state: {out}");
    assert!(out.contains("missing"), "says what it costs you: {out}");
}

#[test]
fn a_clean_answer_is_left_exactly_as_it_was() {
    let b = board(HOUR);
    assert_eq!(mark("All good.", &["stripe"], &b, HOUR), "All good.");
}

#[test]
fn a_source_the_answer_did_not_use_does_not_muddy_it() {
    let mut b = board(HOUR);
    b.get_mut("mail").unwrap().failed(HOUR, "token");
    // mail is broken, but this answer never touched it.
    assert_eq!(mark("All good.", &["stripe"], &b, HOUR), "All good.");
}

#[test]
fn a_source_never_checked_still_marks_the_answer() {
    // Unknown is not the same as fine. An answer built on something that has
    // never once been seen to work should say so, or "I have no idea" reads
    // identically to "everything is healthy".
    let mut b = Board::default();
    b.add(Integration::new("stripe", "Payment notifications stop."));
    let out = mark("You took $40 yesterday.", &["stripe"], &b, HOUR);
    assert!(out.contains("I've never seen stripe work"), "reads as a sentence, not a slot fill: {out}");
}

#[test]
fn several_broken_sources_read_as_a_sentence_rather_than_a_list_of_errors() {
    let mut b = board(HOUR);
    b.get_mut("stripe").unwrap().failed(HOUR, "401");
    b.get_mut("mail").unwrap().failed(HOUR, "token");
    let out = mark("Here's the summary.", &["stripe", "mail"], &b, HOUR);
    assert!(out.contains("stripe is failing, mail is failing"), "{out}");
    assert!(out.contains("they"), "plural reads as a sentence: {out}");
}

#[test]
fn a_name_nobody_registered_is_ignored_rather_than_guessed_at() {
    let b = board(HOUR);
    assert_eq!(mark("All good.", &["nonexistent"], &b, HOUR), "All good.");
}

// ---------------------------------------------------------------------------
// The registry itself.
// ---------------------------------------------------------------------------

#[test]
fn the_board_atlas_actually_runs_with_is_not_empty() {
    // The bug this is here to stop coming back: `Daemon::new` built
    // `Board::default()` and nothing ever added to it, so every reader of the
    // board was reading nothing while looking perfectly healthy.
    let b = dependencies();
    assert!(!b.integrations.is_empty(), "a board nothing registers into cannot report anything");
}

#[test]
fn every_registered_dependency_says_what_breaks_for_you() {
    // A failure has to read as a consequence, not a name. "the language model
    // is failing" is only useful next to what stops working because of it.
    for i in dependencies().integrations {
        assert!(
            !i.if_it_breaks.trim().is_empty(),
            "{} was registered with no consequence written down",
            i.name
        );
    }
}

#[test]
fn a_registered_dependency_starts_unknown_rather_than_working() {
    // Registering something must not assert it works. If it did, the board
    // would open every run claiming health it has not observed.
    let b = dependencies();
    for i in &b.integrations {
        assert_eq!(i.health(0), Health::Unknown, "{} started out claiming health", i.name);
    }
}
