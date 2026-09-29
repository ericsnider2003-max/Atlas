//! How long a turn took, and where it went.

use atlas::timing::*;

fn turn(about: &str, parts: &[(Stage, u32)]) -> Turn {
    let mut t = Turn { ms: vec![], about: about.into() };
    for (s, ms) in parts {
        t.note(*s, *ms);
    }
    t
}

fn ordinary() -> Turn {
    turn(
        "what's on today",
        &[
            (Stage::Listening, 1500),
            (Stage::Hearing, 300),
            (Stage::Understanding, 120),
            (Stage::Doing, 80),
            (Stage::Speaking, 200),
            (Stage::Playing, 1800),
        ],
    )
}

// --- what counts as slowness ------------------------------------------------

#[test]
fn time_spent_listening_and_playing_is_not_atlas_being_slow() {
    // A long question is not a slow assistant, and the audio's real length is
    // not a fault. Counting them would make every long answer look like a
    // problem.
    let t = ordinary();
    assert_eq!(t.atlas_ms(), 700);
    assert!(!Stage::Listening.atlas_controls_it());
    assert!(!Stage::Playing.atlas_controls_it());
}

#[test]
fn the_worst_stage_is_one_atlas_could_have_helped() {
    // Playing took 1800ms and is not the answer.
    assert_eq!(ordinary().worst(), Some((Stage::Hearing, 300)));
}

#[test]
fn a_slow_turn_is_measured_on_the_part_atlas_owns() {
    let t = ordinary();
    assert!(!t.was_slow(FEELS_SLOW_MS), "4s of audio counted as slowness");
    let slow = turn("x", &[(Stage::Hearing, 4000), (Stage::Playing, 500)]);
    assert!(slow.was_slow(FEELS_SLOW_MS));
}

// --- a stage that didn't run isn't a stage that was instant ------------------

#[test]
fn a_stage_that_never_ran_is_absent_rather_than_zero() {
    // A skipped step and an instant one are different facts, and folding them
    // together is how a skipped step looks fast.
    let t = turn("x", &[(Stage::Hearing, 200)]);
    assert_eq!(t.get(Stage::Hearing), Some(200));
    assert_eq!(t.get(Stage::Doing), None);
}

#[test]
fn noting_a_stage_twice_replaces_rather_than_adds() {
    let mut t = turn("x", &[(Stage::Doing, 100)]);
    t.note(Stage::Doing, 250);
    assert_eq!(t.get(Stage::Doing), Some(250));
    assert_eq!(t.atlas_ms(), 250);
}

// --- the window -------------------------------------------------------------

#[test]
fn the_typical_turn_is_the_middle_one_not_the_mean() {
    // A mean is dragged around by one bad turn and describes neither the usual
    // case nor the worst.
    let mut r = Recent::default();
    for ms in [100, 110, 120, 130, 9000] {
        r.add(turn("x", &[(Stage::Doing, ms)]));
    }
    assert_eq!(r.typical_ms(), Some(120), "one bad turn moved the typical one");
}

#[test]
fn only_the_recent_window_is_kept() {
    // A bad afternoon should show, not be diluted by a good month.
    let mut r = Recent::default();
    for i in 0..(KEEP + 20) {
        r.add(turn(&format!("{i}"), &[(Stage::Doing, 100)]));
    }
    assert_eq!(r.turns.len(), KEEP);
}

#[test]
fn slow_turns_are_listed_worst_first() {
    let mut r = Recent::default();
    r.add(turn("quick", &[(Stage::Doing, 50)]));
    r.add(turn("bad", &[(Stage::Doing, 9000)]));
    r.add(turn("worse", &[(Stage::Doing, 12000)]));
    let slow = r.slow_ones();
    assert_eq!(slow.len(), 2);
    assert_eq!(slow[0].about, "worse");
}

#[test]
fn the_costliest_stage_is_summed_not_averaged() {
    // A stage that's slow every time costs more than one that's occasionally
    // terrible, and summing says so.
    let mut r = Recent::default();
    for _ in 0..10 {
        r.add(turn("x", &[(Stage::Hearing, 400), (Stage::Doing, 50)]));
    }
    r.add(turn("y", &[(Stage::Doing, 3000)]));
    assert_eq!(r.worst_stage().map(|(s, _)| s), Some(Stage::Hearing));
}

// --- what it says -----------------------------------------------------------

#[test]
fn asked_why_it_is_slow_it_names_the_stage_rather_than_listing_numbers() {
    // Six numbers is something you have to interpret. One sentence is an
    // answer.
    let mut r = Recent::default();
    for _ in 0..6 {
        r.add(turn("x", &[(Stage::Hearing, 5000)]));
    }
    let said = r.why_slow();
    assert!(said.contains(Stage::Hearing.plain()), "got: {said}");
    // One number is an answer; six is a table you have to interpret. The
    // typical turn time earns its place — the per-stage breakdown does not.
    let stages_named = Stage::all().iter().filter(|s| said.contains(s.plain())).count();
    assert_eq!(stages_named, 1, "it listed the whole breakdown: {said}");
}

#[test]
fn nothing_slow_says_so_rather_than_inventing_a_culprit() {
    let mut r = Recent::default();
    for _ in 0..5 {
        r.add(ordinary());
    }
    assert!(r.why_slow().contains("Nothing's been slow"));
}

#[test]
fn too_little_data_says_so_rather_than_guessing() {
    assert!(Recent::default().why_slow().contains("haven't handled enough"));
}

// --- the signal that never had a producer -----------------------------------

#[test]
fn getting_slower_over_the_window_produces_the_signal() {
    // `selfaudit::Kind::GotSlower` has been in the taxonomy since it was
    // written with nothing able to raise it. A signal nothing can produce is a
    // signal that will never fire.
    let mut r = Recent::default();
    for _ in 0..6 {
        r.add(turn("x", &[(Stage::Doing, 200)]));
    }
    for _ in 0..6 {
        r.add(turn("x", &[(Stage::Doing, 900)]));
    }
    let s = r.got_slower().expect("it got four times slower and said nothing");
    assert_eq!(s.kind, atlas::selfaudit::Kind::GotSlower);
    assert!(s.example.contains("200") && s.example.contains("900"));
}

#[test]
fn a_steady_machine_produces_no_signal() {
    let mut r = Recent::default();
    for _ in 0..14 {
        r.add(turn("x", &[(Stage::Doing, 200)]));
    }
    assert!(r.got_slower().is_none());
}

#[test]
fn a_small_wobble_is_not_a_regression() {
    // Turn times are noisy. A few percent means nothing, so the bar is half
    // again.
    let mut r = Recent::default();
    for _ in 0..6 {
        r.add(turn("x", &[(Stage::Doing, 200)]));
    }
    for _ in 0..6 {
        r.add(turn("x", &[(Stage::Doing, 240)]));
    }
    assert!(r.got_slower().is_none(), "noise was reported as a regression");
}

#[test]
fn too_few_turns_is_not_evidence_of_anything() {
    let mut r = Recent::default();
    for _ in 0..4 {
        r.add(turn("x", &[(Stage::Doing, 100)]));
    }
    r.add(turn("x", &[(Stage::Doing, 9000)]));
    assert!(r.got_slower().is_none(), "five turns was treated as a trend");
}

#[test]
fn every_stage_can_say_what_it_is_in_plain_words() {
    for s in Stage::all() {
        // "doing it" is eight characters and is the right answer, so the bar
        // is that it reads as words rather than that it is long.
        assert!(s.plain().contains(' '), "{s:?} isn't phrased as words: {}", s.plain());
        assert!(s.plain().chars().all(|c| c.is_lowercase() || c == ' '));
    }
}

// ===========================================================================
// An unreadable models folder is not an empty one
// ===========================================================================

#[test]
fn a_missing_models_folder_says_so_rather_than_reporting_none_installed() {
    // "No models installed" is the message you get when the folder is missing,
    // when the path is wrong, and when a permission is off. Three problems,
    // one answer — and the answer sends you to download what you already have.
    let (reg, trouble) =
        atlas::models::Registry::scan_reporting(std::path::Path::new("/definitely/not/here"));
    assert!(reg.models.is_empty());
    let why = trouble.expect("an unreadable folder read as an empty one");
    assert!(why.contains("doesn't exist"), "got: {why}");
    assert!(
        why.contains("different from having none installed"),
        "it didn't distinguish the two: {why}"
    );
}

#[test]
fn a_readable_but_empty_folder_reports_no_trouble() {
    // Genuinely empty is genuinely empty. The check must not manufacture a
    // problem where there isn't one.
    let d = std::env::temp_dir().join(format!("atlas-models-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&d);
    let (reg, trouble) = atlas::models::Registry::scan_reporting(&d);
    let _ = std::fs::remove_dir_all(&d);
    assert!(reg.models.is_empty());
    assert!(trouble.is_none(), "an empty folder was reported as a fault: {trouble:?}");
}

#[test]
fn a_file_that_will_not_open_is_named_as_a_bad_download() {
    // The one case where re-downloading actually helps, so it's worth saying
    // separately from the folder problems.
    let d = std::env::temp_dir().join(format!("atlas-models-bad-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&d);
    std::fs::write(d.join("half.gguf"), b"not a real gguf").unwrap();
    let (reg, trouble) = atlas::models::Registry::scan_reporting(&d);
    let _ = std::fs::remove_dir_all(&d);
    assert!(reg.models.is_empty());
    assert!(trouble.unwrap().contains("didn't finish"));
}
