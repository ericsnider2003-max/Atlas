//! A field that decides something, and nothing that ever writes it.
//!
//! ## What was wrong
//!
//! `pipeline::Work` has six fields — `stage`, `thought`, `build`, `review`,
//! `refinements`, `rounds`. **Not one of them was ever written anywhere in
//! `src/`.** `Work::new` set `stage: Stage::Thought` and the rest to
//! `None`/empty, and no production code changed any of them again, ever.
//!
//! Everything else follows from that single fact:
//!
//! * `Session::may_start` refuses while `thought` is `None`, so
//!   `Intent::WorkOnYourself("fix the thing")` answered *"Before I touch
//!   anything: no diagnosis — what's actually wrong, and what would prove it
//!   fixed?"* — for every input, every time, with no way to answer it.
//! * `Work::what_next` could only take its `Stage::Thought` arm. So
//!   `Next::Do(Stage::Build)`, `Next::Blocked`, `Next::Land` and
//!   `Next::HandOver` were all unreachable, and with `Next::Land` went
//!   `Daemon::land_it`, `selfwork::what_holds_it_back`, `selfwork::land`, the
//!   four landing checks, `mend::paper_overs` and `selfgrant::may_land`.
//! * `Session::after`, `Session::begin`, `Session::take_it_elsewhere`,
//!   `selfwork::run_tests`, `Tried::worth_showing`, `pipeline::review`,
//!   `refinement_is_warranted`, `Concern::blocks`, `Review::clean` and the
//!   whole `strategy` ladder had no production caller either — nothing could
//!   get past stage one to call them.
//! * `PipelineConfig::max_rounds` was compared against `rounds`, which was
//!   always `0`.
//! * `Sandbox::create` has no production caller and `Daemon::pending_landing`
//!   is never pushed to, which is the same hole seen from the other end.
//!
//! So *"Atlas fixes faulty code in a sandbox, and if it passes it gets fixed
//! then and there"* had every piece written, tested and correct, and no way
//! in. An earlier pass concluded the missing part was the last four words of
//! that sentence. The missing part was the first half.
//!
//! ## Why every existing guard missed it
//!
//! `wiring.rs`, `dead_methods.rs` and `dead_capabilities.rs` all ask the same
//! question — *can the program reach this code?* — and the answer was yes.
//! `WorkOnYourself` is a real intent that really dispatches and really calls
//! `may_start` and `what_next`. Both functions ran, on every such command.
//!
//! What no guard asked is whether a **field those functions branch on** is
//! ever given a value. A state machine whose state is never assigned is
//! reachable code with an unreachable behaviour, and reachability of code
//! does not imply reachability of outcome.
//!
//! `one_name_one_record.rs` asks that question one level up — it found
//! `budget_ledger` and `phone_mirror` read and never written. This file is
//! the same question inside a struct.
//!
//! ## What this pins
//!
//! For each field below: that production code outside its own module both
//! *can* write it (a method exists) and *does* (something calls that method).
//! Named list rather than a count, because a count is satisfied by editing
//! the number.

use std::collections::BTreeSet;

/// A field that a decision reads, the advancing method that writes it, and
/// what goes wrong when nothing calls that method.
///
/// The third column is the point. A guard that says "this is unwired" and not
/// "and here is what stops working" gets read as pedantry and suppressed.
const DECIDED_ON: &[(&str, &str, &str)] = &[
    (
        "stage",
        "record_thought",
        "`what_next` can only take its `Stage::Thought` arm, so `Next::Land` — and \
         `land_it`, the four landing checks and the grant — is unreachable",
    ),
    (
        "thought",
        "record_thought",
        "`Session::may_start` refuses for ever, so `WorkOnYourself` answers \"no \
         diagnosis\" to every input with no way to answer it",
    ),
    (
        "build",
        "record_build",
        "the Build stage can never produce its artefact, so the proving test's result \
         is never recorded",
    ),
    (
        "review",
        "record_review",
        "`Work::may_land` refuses with \"not reviewed\", and `pipeline::review` has \
         nothing to review",
    ),
    (
        "refinements",
        "record_refinement",
        "the review–refine loop cannot go round, so a blocking review is terminal",
    ),
    (
        "rounds",
        "record_review",
        "`max_rounds` is compared against a number that is always zero, so the \
         hand-over that stops a loop repeating for ever never fires",
    ),
];

/// Does this line **assign** to `<field>`, rather than compare against it?
///
/// `contains("work.stage =")` was the first version and `work.stage ==` also
/// contains it — so `if s.work.stage == Stage::Abandoned`, which is a read,
/// was reported as the machinery being bypassed. A guard whose output
/// includes the code that respects it is a guard that gets switched off.
fn assigns(line: &str, field: &str) -> bool {
    let mut rest = line;
    while let Some(i) = rest.find(field) {
        let after = rest[i + field.len()..].trim_start();
        // `= x` is an assignment; `==` and `!=` are comparisons, and the `!`
        // of `!=` sits before the field so only `==` can appear here.
        if let Some(tail) = after.strip_prefix('=') {
            if !tail.starts_with('=') {
                return true;
            }
        }
        rest = &rest[i + field.len()..];
    }
    false
}

/// Every line of `src/`, with each file's unit tests cut off.
///
/// Everything below `#[cfg(test)]` in a file is that file's own tests, and a
/// method called only from there is not wired.
fn production_lines() -> Vec<(String, usize, String)> {
    let mut out = Vec::new();
    let mut stack = vec![std::path::PathBuf::from("src")];
    while let Some(p) = stack.pop() {
        if p.is_dir() {
            for e in std::fs::read_dir(&p).expect("src is readable").flatten() {
                stack.push(e.path());
            }
            continue;
        }
        if p.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let text = std::fs::read_to_string(&p).unwrap_or_default();
        let live = match text.find("#[cfg(test)]") {
            Some(at) => &text[..at],
            None => &text[..],
        };
        for (i, line) in live.lines().enumerate() {
            let t = line.trim_start();
            // Comments and doc comments are where this file's own findings are
            // written down. Counting them as callers would let the note about
            // a gap close the gap.
            if t.starts_with("//") || t.starts_with("///") || t.starts_with("*") {
                continue;
            }
            out.push((p.display().to_string(), i + 1, line.to_string()));
        }
    }
    out
}

#[test]
fn every_field_a_decision_reads_has_a_method_that_writes_it() {
    let pipeline = std::fs::read_to_string("src/pipeline.rs").expect("src/pipeline.rs");
    let live = match pipeline.find("#[cfg(test)]") {
        Some(at) => &pipeline[..at],
        None => &pipeline[..],
    };
    let mut missing = Vec::new();
    for (field, writer, breaks) in DECIDED_ON {
        if !live.contains(&format!("fn {writer}")) {
            missing.push(format!("{field} (no `{writer}`) — {breaks}"));
        }
    }
    assert!(
        missing.is_empty(),
        "`pipeline::Work` has no way to write these fields, so they keep whatever \
         `Work::new` gave them for the life of the session:\n  {}",
        missing.join("\n  ")
    );
}

/// The advances that are still waiting on something that does not exist, and
/// what that something is.
///
/// Not a suppression list. It is checked in **both directions**: an entry
/// whose writer has acquired a caller fails, so the entry is deleted by the
/// change that makes it false rather than surviving it — the same shape as
/// `budget::untracked`.
///
/// ## What is missing, once, so it is not restated three times
///
/// This used to say the Build stage had nothing to feed it — no candidate
/// writer. That was closed on 20 Sep: `Daemon::attempt_own_fix` drafts a fix
/// (`selfwork::draft_fix`), proves it in a copy of the tree (`prove_in_a_copy`
/// -> `Sandbox::create`, `run_tests`, `run_the_proof`), pushes the change onto
/// `pending_landing`, and calls `record_build` and `record_review`. So Build
/// and Review both have writers now.
///
/// `record_refinement` got its writer on 25 Sep 2026 (Eric's ruling E2):
/// `refine_own_fix` answers a review with a fixable concern by taking another
/// pass, so every advance has a writer now.
const STILL_WAITING_ON_A_CODE_WRITER: &[(&str, &str)] = &[
];

#[test]
fn the_advancing_methods_are_actually_called_from_somewhere_else() {
    // The half that matters. A `record_thought` that exists and is called by
    // nothing leaves the field exactly as unwritten as before, and reads as a
    // fix in a diff.
    let lines = production_lines();
    let writers: BTreeSet<&str> = DECIDED_ON.iter().map(|(_, w, _)| *w).collect();

    let called_from = |w: &str| -> Vec<String> {
        lines
            .iter()
            .filter(|(file, _, line)| {
                // Outside `pipeline.rs`, because a method one `impl Work`
                // block calls from another is still inside the machinery.
                !file.ends_with("pipeline.rs") && line.contains(&format!(".{w}("))
            })
            .map(|(file, n, _)| format!("{file}:{n}"))
            .collect()
    };

    let mut uncalled = Vec::new();
    for w in &writers {
        if STILL_WAITING_ON_A_CODE_WRITER.iter().any(|(name, _)| name == w) {
            continue;
        }
        if called_from(w).is_empty() {
            let breaks = DECIDED_ON
                .iter()
                .find(|(_, writer, _)| writer == w)
                .map(|(_, _, b)| *b)
                .unwrap_or("");
            uncalled.push(format!("{w} — {breaks}"));
        }
    }
    assert!(
        uncalled.is_empty(),
        "these exist and nothing outside `pipeline.rs` calls them, so the fields they \
         write are still never written:\n  {}",
        uncalled.join("\n  ")
    );
}

#[test]
fn the_list_of_what_is_still_waiting_is_still_true() {
    // Both directions, so the list cannot become a place gaps go to be
    // forgotten. The moment something writes a candidate fix and calls
    // `record_build`, this fails and the entry has to go — which is the
    // point.
    let lines = production_lines();
    let mut now_wired = Vec::new();
    for (w, why) in STILL_WAITING_ON_A_CODE_WRITER {
        let callers: Vec<String> = lines
            .iter()
            .filter(|(file, _, line)| {
                !file.ends_with("pipeline.rs") && line.contains(&format!(".{w}("))
            })
            .map(|(file, n, _)| format!("{file}:{n}"))
            .collect();
        if !callers.is_empty() {
            now_wired.push(format!("{w} (listed as: {why}) is now called from {}", callers.join(", ")));
        }
    }
    assert!(
        now_wired.is_empty(),
        "`STILL_WAITING_ON_A_CODE_WRITER` says these have no caller and they do. Delete \
         the entries — the note is the thing that is now false:\n  {}",
        now_wired.join("\n  ")
    );
}

#[test]
fn the_build_stage_reaches_for_a_fix_rather_than_answering_with_its_own_name() {
    // The failure this guards against is the one an earlier pass made: a stage
    // that answers with its own name ("building it next") reads as progress and
    // is a dead end. Now that the loop is closed, the check is the mirror of
    // the old one: the daemon must actually drive the fix at Build, and must
    // not still carry the old "nothing writes a candidate fix" dead end.
    let daemon = crate::common::source_of("daemon");
    assert!(
        daemon.contains("fn attempt_own_fix"),
        "the Build stage has no candidate writer wired — the loop is open again"
    );
    assert!(
        !daemon.contains("nothing here writes a candidate fix"),
        "the old dead-end message is still in the tree though the loop is closed"
    );
}

#[test]
fn the_diagnosis_can_actually_be_supplied() {
    // The specific hole, pinned at the level a person would notice it: there
    // has to be a way to answer the question Atlas asks. `Diagnosing` is that
    // way, and it is only worth anything if the daemon drives it.
    let lines = production_lines();
    for (what, needle) in [
        ("the questions are asked", ".still_needs("),
        ("an answer is taken", ".heard("),
        ("the proving test is run", "run_the_proof("),
        ("the diagnosis is accepted", ".accept_diagnosis("),
    ] {
        let found: Vec<String> = lines
            .iter()
            .filter(|(file, _, line)| {
                !file.ends_with("selfwork.rs") && !file.ends_with("pipeline.rs")
                    && line.contains(needle)
            })
            .map(|(file, n, _)| format!("{file}:{n}"))
            .collect();
        assert!(
            !found.is_empty(),
            "nothing outside `selfwork.rs`/`pipeline.rs` does this: {what} ({needle}). \
             Without it `Session::may_start` refuses for ever and \
             `Intent::WorkOnYourself` cannot get past its first sentence."
        );
    }
}

#[test]
fn the_stage_is_a_consequence_and_not_a_setting() {
    // The cheap way to satisfy the two tests above is a `set_stage(Stage)`,
    // which is the same hole with a nicer name: a caller that can assign
    // `Stage::Implement` has skipped the thought, the build and the review,
    // and `may_land` is the only thing left standing between it and copying
    // files over yours.
    let pipeline = std::fs::read_to_string("src/pipeline.rs").expect("src/pipeline.rs");
    let live = match pipeline.find("#[cfg(test)]") {
        Some(at) => &pipeline[..at],
        None => &pipeline[..],
    };
    for shape in ["pub fn set_stage", "pub fn stage_mut", "pub fn advance_to"] {
        assert!(
            !live.contains(shape),
            "`{shape}` lets a caller name the stage it wants. Every advance has to be \
             something an accepted artefact caused."
        );
    }

    // And nothing outside `pipeline.rs` assigns a `Work`'s stage directly.
    //
    // Narrowed to a `work` accessor rather than any field called `stage`:
    // `mind.rs` has its own unrelated `Stage` and `self.stage = stage` in it,
    // and counting that would make this guard report a file it is not about —
    // which is how a guard gets read as noise and switched off.
    let assigned: Vec<String> = production_lines()
        .into_iter()
        .filter(|(file, _, line)| {
            !file.ends_with("pipeline.rs")
                // An assignment or a struct literal, never a `match` arm —
                // reading the stage to decide what to say is the whole point
                // of having one.
                && (assigns(line, "work.stage")
                    || line.contains("stage: crate::pipeline::Stage")
                    || line.contains("stage: pipeline::Stage"))
        })
        .map(|(file, n, line)| format!("{file}:{n}: {}", line.trim()))
        .collect();
    assert!(
        assigned.is_empty(),
        "the stage is assigned from outside the machinery that earns it:\n  {}",
        assigned.join("\n  ")
    );
}

// ===================== the behaviour, not just the wiring ================

use atlas::pipeline::{Build, Concern, Diagnosing, Note, Review, Stage, Work};

fn a_real_diagnosis() -> Diagnosing {
    let mut d = Diagnosing::default();
    assert!(d.answer("the settings panel takes eight seconds to open").is_some());
    assert!(d.answer("every row re-reads the whole config file from disk").is_some());
    assert!(d.answer("src/panel.rs").is_some());
    assert!(d.answer("the_panel_opens_without_rereading_the_config").is_none());
    d
}

#[test]
fn four_answers_and_a_failing_proof_gets_past_stage_one() {
    let mut w = Work::new("the settings panel is slow");
    assert_eq!(w.stage, Stage::Thought, "a new piece of work starts at the thinking");

    let t = a_real_diagnosis().into_thought(true, Vec::new()).expect("all four parts");
    w.record_thought(t).expect("a real diagnosis was refused");

    assert_eq!(w.stage, Stage::Build, "a diagnosis that holds up did not advance the stage");
    assert!(w.thought.is_some(), "the diagnosis was not kept");
}

#[test]
fn a_proof_that_passes_already_does_not_get_past_stage_one() {
    // The single most valuable check in the loop: a proving test that passes
    // before the change is testing something else.
    let mut w = Work::new("the settings panel is slow");
    let t = a_real_diagnosis().into_thought(false, Vec::new()).expect("all four parts");
    let err = w.record_thought(t).expect_err("a proof that already passes was accepted");
    assert!(err.contains("passes already"), "the refusal doesn't say why: {err}");
    assert_eq!(w.stage, Stage::Thought, "it advanced anyway");
    assert!(w.thought.is_none(), "a refused diagnosis was kept");
}

#[test]
fn a_cause_that_restates_the_symptom_does_not_get_past_stage_one() {
    let mut d = Diagnosing::default();
    d.answer("the settings panel is too slow to open");
    d.answer("the settings panel takes too long to open");
    d.answer("src/panel.rs");
    d.answer("a_test");
    let mut w = Work::new("slow panel");
    let err = w
        .record_thought(d.into_thought(true, Vec::new()).expect("four parts"))
        .expect_err("a restated symptom was accepted as a diagnosis");
    assert!(err.contains("restatement"), "got: {err}");
}

#[test]
fn the_whole_ladder_can_be_climbed_and_ends_somewhere_that_lands() {
    let mut w = Work::new("the settings panel is slow");
    w.record_thought(a_real_diagnosis().into_thought(true, Vec::new()).unwrap()).unwrap();

    // Build.
    let good = Build {
        touched: vec!["src/panel.rs".into()],
        tests_before: 100,
        tests_after: 101,
        proof_passes: true,
        nothing_else_broke: true,
    };
    w.record_build(good.clone()).expect("a good build was refused");
    assert_eq!(w.stage, Stage::Review);

    // Review, using the real reviewer against the real thought.
    let r = atlas::pipeline::review(w.thought.as_ref().unwrap(), &good, &[]);
    assert!(r.clean(), "a fix in the place the cause was said to be was not clean: {r:?}");
    w.record_review(r).expect("a clean review was refused");
    assert_eq!(w.stage, Stage::Implement);

    // And that is the one stage `what_next` lands from.
    let cfg = atlas::pipeline::PipelineConfig::default();
    assert!(
        matches!(w.what_next(&cfg), atlas::pipeline::Next::Land(_)),
        "the end of the ladder is not a landing: {:?}",
        w.what_next(&cfg)
    );
    w.may_land().expect("every stage has an artefact and it still refused");
}

#[test]
fn a_build_whose_proof_still_fails_does_not_reach_review() {
    let mut w = Work::new("the settings panel is slow");
    w.record_thought(a_real_diagnosis().into_thought(true, Vec::new()).unwrap()).unwrap();
    let err = w
        .record_build(Build {
            touched: vec!["src/panel.rs".into()],
            tests_before: 100,
            tests_after: 100,
            proof_passes: false,
            nothing_else_broke: true,
        })
        .expect_err("a build whose proving test still fails reached review");
    assert!(err.contains("proving test still fails"), "got: {err}");
    assert_eq!(w.stage, Stage::Build, "it advanced on a failing proof");
}

#[test]
fn a_build_that_broke_something_else_does_not_reach_review() {
    let mut w = Work::new("the settings panel is slow");
    w.record_thought(a_real_diagnosis().into_thought(true, Vec::new()).unwrap()).unwrap();
    let err = w
        .record_build(Build {
            touched: vec!["src/panel.rs".into()],
            tests_before: 100,
            tests_after: 100,
            proof_passes: true,
            nothing_else_broke: false,
        })
        .expect_err("a build that broke something else reached review");
    assert!(err.contains("something else broke"), "got: {err}");
}

#[test]
fn a_blocking_review_goes_round_and_counts_the_round() {
    // `max_rounds` was compared against a number that was always zero, so the
    // hand-over that stops a loop repeating for ever could never fire.
    let mut w = Work::new("the settings panel is slow");
    w.record_thought(a_real_diagnosis().into_thought(true, Vec::new()).unwrap()).unwrap();
    w.record_build(Build {
        touched: vec!["src/elsewhere.rs".into()],
        tests_before: 100,
        tests_after: 100,
        proof_passes: true,
        nothing_else_broke: true,
    })
    .unwrap();

    let blocked = Review {
        notes: vec![Note {
            kind: Concern::NotWhereTheCauseWas,
            what: "the cause was in src/panel.rs and nothing there changed".into(),
        }],
    };
    let err = w.record_review(blocked).expect_err("a blocking review was treated as clean");
    assert!(err.contains("src/panel.rs"), "the reason isn't the blocker: {err}");
    assert_eq!(w.stage, Stage::Refine, "a blocking review did not go round");
    assert_eq!(w.rounds, 1, "the round was not counted, so `max_rounds` means nothing");
}

#[test]
fn a_refinement_sends_it_back_to_build_rather_than_to_the_old_review() {
    // A refinement is a change, and a change has to prove itself again.
    // Refining into a review written about different code is how a loop like
    // this launders a second fault past a first review.
    let mut w = Work::new("the settings panel is slow");
    w.record_thought(a_real_diagnosis().into_thought(true, Vec::new()).unwrap()).unwrap();
    w.record_build(Build {
        touched: vec!["src/elsewhere.rs".into()],
        tests_before: 100,
        tests_after: 100,
        proof_passes: true,
        nothing_else_broke: true,
    })
    .unwrap();
    let _ = w.record_review(Review {
        notes: vec![Note {
            kind: Concern::NotWhereTheCauseWas,
            what: "nothing in src/panel.rs changed".into(),
        }],
    });

    w.record_refinement(atlas::pipeline::Refinement {
        answers: Concern::NotWhereTheCauseWas,
        what_changed: "moved the fix into src/panel.rs".into(),
    })
    .expect("a refinement answering a real review note was refused");
    assert_eq!(w.stage, Stage::Build, "it went somewhere other than back to building");
    assert!(w.review.is_none(), "the old review survived a change it was not written about");
}

#[test]
fn a_refinement_that_answers_nothing_in_the_review_is_refused() {
    let mut w = Work::new("the settings panel is slow");
    w.record_thought(a_real_diagnosis().into_thought(true, Vec::new()).unwrap()).unwrap();
    w.record_build(Build {
        touched: vec!["src/elsewhere.rs".into()],
        tests_before: 100,
        tests_after: 100,
        proof_passes: true,
        nothing_else_broke: true,
    })
    .unwrap();
    let _ = w.record_review(Review {
        notes: vec![Note {
            kind: Concern::NotWhereTheCauseWas,
            what: "nothing in src/panel.rs changed".into(),
        }],
    });
    let before = w.stage;
    let err = w
        .record_refinement(atlas::pipeline::Refinement {
            answers: Concern::HardToFollow,
            what_changed: "renamed some things".into(),
        })
        .expect_err("scope creep wearing a hat was accepted as a refinement");
    assert!(err.contains("new change"), "got: {err}");

    // The behaviour, not the sentence: nothing was recorded and nothing
    // moved. A refusal that still stores the refinement and still advances
    // the stage is a refusal in name only.
    assert!(w.refinements.is_empty(), "a refused refinement was recorded anyway");
    assert_eq!(w.stage, before, "a refused refinement moved the work on");
    assert!(w.review.is_some(), "a refused refinement cleared the review it did not answer");

    // And one that DOES answer the note still works afterwards, so the
    // refusal is not just "refuse everything".
    w.record_refinement(atlas::pipeline::Refinement {
        answers: Concern::NotWhereTheCauseWas,
        what_changed: "moved the fix into src/panel.rs".into(),
    })
    .expect("a refinement answering the review note was refused too");
    assert_eq!(w.refinements.len(), 1);
    assert_eq!(w.stage, Stage::Build);
}

#[test]
fn an_artefact_cannot_be_recorded_for_a_stage_it_is_not_at() {
    // Otherwise the ladder can be climbed sideways: a build recorded at
    // Thought would sit there while `what_next` still asked for a diagnosis,
    // and a second `record_thought` would overwrite a diagnosis the review
    // was written against.
    let mut w = Work::new("x");
    assert!(w.record_build(Build {
        touched: vec![],
        tests_before: 0,
        tests_after: 0,
        proof_passes: true,
        nothing_else_broke: true,
    }).is_err(), "a build was recorded before anything said what was wrong");
    assert!(
        w.record_review(Review { notes: vec![] }).is_err(),
        "a review was recorded before there was anything to review"
    );

    w.record_thought(a_real_diagnosis().into_thought(true, Vec::new()).unwrap()).unwrap();
    let err = w
        .record_thought(a_real_diagnosis().into_thought(true, Vec::new()).unwrap())
        .expect_err("a second diagnosis replaced the first");
    assert!(err.contains("past the thinking"), "got: {err}");
}

// ===================== collecting the four answers =======================

#[test]
fn it_asks_for_all_four_parts_and_stops_asking() {
    let mut d = Diagnosing::default();
    let mut asked = Vec::new();
    for answer in ["a symptom", "a different cause", "src/somewhere.rs", "a_named_test"] {
        asked.push(d.next_question().expect("it stopped asking too early"));
        d.answer(answer);
    }
    assert_eq!(asked.len(), 4);
    assert!(d.next_question().is_none(), "it kept asking after four answers");
    // Four different questions, not one repeated.
    let unique: BTreeSet<&&str> = asked.iter().collect();
    assert_eq!(unique.len(), 4, "it asked the same thing twice: {asked:?}");
}

#[test]
fn a_blank_turn_does_not_fill_a_part_with_nothing() {
    // `is_thought_through` checks for empty strings, and a required part
    // filled with `""` would pass "has an answer" and fail the emptiness
    // check with a confusing reason — or, for `where_`, pass both and make
    // the review compare the change against an empty location.
    let mut d = Diagnosing::default();
    let first = d.next_question().expect("a question");
    assert_eq!(d.answer("   "), Some(first), "a blank answer was taken as the symptom");
    assert!(d.symptom.is_none());
}

#[test]
fn the_proving_test_is_not_something_the_answers_can_assert() {
    // `proof_fails_now` is the measurement the whole discipline rests on, and
    // the one a person would answer wrongly in good faith. `Diagnosing` has
    // no slot for it: it arrives as an argument, from having run the test.
    let d = a_real_diagnosis();
    assert!(d.into_thought(true, Vec::new()).unwrap().proof_fails_now);
    assert!(!d.into_thought(false, Vec::new()).unwrap().proof_fails_now);

    let src = std::fs::read_to_string("src/pipeline.rs").expect("src/pipeline.rs");
    let struct_body = src
        .split("pub struct Diagnosing")
        .nth(1)
        .and_then(|s| s.split('}').next())
        .expect("the Diagnosing struct");
    assert!(
        !struct_body.contains("proof_fails_now"),
        "`Diagnosing` collects `proof_fails_now` as an answer. It is a measurement: \
         a diagnosis that asserts its own proof fails has not been checked."
    );
}

// ===================== reading a real test run ===========================

use atlas::selfwork::{read_a_proof_run, ProofToday};

#[test]
fn a_failing_proof_is_read_as_failing() {
    let out = "running 1 test\ntest the_thing ... FAILED\n\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 4931 filtered out\n";
    assert_eq!(read_a_proof_run(out), ProofToday::Fails);
}

#[test]
fn a_passing_proof_is_read_as_already_passing() {
    let out = "running 1 test\ntest the_thing ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 4931 filtered out\n";
    assert_eq!(read_a_proof_run(out), ProofToday::PassesAlready);
}

#[test]
fn a_filter_that_matched_nothing_is_not_read_as_passing() {
    // The one that matters most. `cargo test a_name_that_does_not_exist`
    // exits **zero** and prints `ok.` for every target, so anything reading
    // the exit status — or the word "ok" — concludes the proving test passes
    // already and refuses to start work on a real fault.
    let out = "running 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 4932 filtered out\n\
               running 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 12 filtered out\n";
    assert_eq!(
        read_a_proof_run(out),
        ProofToday::NotWrittenYet,
        "a test that does not exist yet was reported as one that passes"
    );
}

#[test]
fn many_targets_are_added_up_rather_than_last_one_wins() {
    // The suite runs one binary per test file, so the failure can be in the
    // third of two hundred `test result:` lines.
    let out = "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 9 filtered out\n\
               test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 3 filtered out\n\
               test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 7 filtered out\n";
    assert_eq!(read_a_proof_run(out), ProofToday::Fails);
}

/// Research report item 5 (1 Oct 2026): other runners' results are read,
/// and a test program that died before its summary is never a pass.
#[test]
fn every_runner_is_read_and_a_crash_is_never_a_pass() {
    use atlas::selfwork::{count_passing, read_run};
    // pytest
    assert_eq!(read_a_proof_run("tests/test_a.py .F\n===== 1 failed, 1 passed in 0.12s ====="), ProofToday::Fails);
    assert_eq!(read_a_proof_run("===== 4 passed in 0.10s ====="), ProofToday::PassesAlready);
    // Jest / Vitest
    assert_eq!(read_a_proof_run("Tests:       1 failed, 5 passed, 6 total"), ProofToday::Fails);
    assert_eq!(count_passing("Tests:       5 passed, 5 total"), 5);
    // node --test / TAP
    assert_eq!(read_a_proof_run("# tests 3\n# pass 3\n# fail 0\n"), ProofToday::PassesAlready);
    assert_eq!(read_a_proof_run("ℹ tests 3\nℹ pass 2\nℹ fail 1\n"), ProofToday::Fails);
    // go
    assert_eq!(read_a_proof_run("--- FAIL: TestThing (0.00s)\nFAIL\n"), ProofToday::Fails);
    // A cargo test program killed part way: one summary for two programs.
    let crashed = "     Running unittests src/lib.rs (target/debug/deps/atlas-1)\n\
                   test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n\
                        Running tests/all.rs (target/debug/deps/all-2)\n\
                   thread 'main' has overflowed its stack\n";
    assert!(read_run(crashed).crashed);
    assert_eq!(read_a_proof_run(crashed), ProofToday::Fails, "ten passes and a crash is not passing");
    assert_eq!(read_run(crashed).passed, 10);
    // Nothing anyone recognises is still "no test yet", never a pass.
    assert_eq!(read_a_proof_run("all good!"), ProofToday::NotWrittenYet);
}

#[test]
fn a_tree_that_does_not_build_says_nothing_either_way() {
    // The pairs matter more than the wording: the SAME result lines, with and
    // without the compiler error, have to be read differently. Without this
    // the guard would pass on a function that returned `CouldNotRun` for
    // everything.
    let result_line =
        "test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 4931 filtered out\n";
    for broken in [
        "error[E0425]: cannot find value `x` in this scope\nerror: could not compile `atlas`\n",
        "error: could not compile `atlas` (test \"thing\") due to 1 previous error\n",
    ] {
        let with_error = format!("{broken}{result_line}");
        match read_a_proof_run(&with_error) {
            ProofToday::CouldNotRun(why) => {
                assert!(why.contains("doesn't build"), "got: {why}")
            }
            o => panic!("a broken build was read as evidence about the proof: {o:?}"),
        }
        // The identical run without the compiler error is a real answer.
        assert_eq!(
            read_a_proof_run(result_line),
            ProofToday::Fails,
            "the same result line without a compiler error stopped being readable, so \
             the build check is swallowing everything"
        );
    }
}

#[test]
fn the_wait_for_a_proof_is_bounded() {
    // This runs on the tick thread — the one that listens, answers, polls and
    // refreshes the instance lock, whose staleness window is 150s. An
    // unbounded `cargo test` here lets a second Atlas read the lock as
    // abandoned and take it, and two of them then write the same state
    // folder. Same reasoning as `workspace::BRINGUP_BUDGET_SECS`.
    assert!(
        atlas::selfwork::PROOF_BUDGET_SECS < 150,
        "the proving test may run for {}s and the lock goes stale at 150s",
        atlas::selfwork::PROOF_BUDGET_SECS
    );
    assert!(
        atlas::selfwork::PROOF_BUDGET_SECS >= 30,
        "30s doesn't compile a test binary, so every proof would come back as \
         'still running'"
    );
}

#[test]
fn the_assignment_check_can_tell_an_assignment_from_a_comparison() {
    // Because the guard above is only worth having if it reports the right
    // lines, and it did not: `work.stage ==` contains `work.stage =`, so
    // every read of the stage was reported as a bypass of the machinery.
    assert!(assigns("    self.work.stage = Stage::Done;", "work.stage"));
    assert!(assigns("s.work.stage=Stage::Done;", "work.stage"));
    assert!(!assigns("if s.work.stage == Stage::Abandoned {", "work.stage"));
    assert!(!assigns("if s.work.stage != Stage::Abandoned {", "work.stage"));
    assert!(!assigns("            s.work.stage,", "work.stage"));
    // Comments are not this function's job — `production_lines` drops them
    // before anything gets here, which is why the note in `selfwork.rs`
    // describing the bug does not count as the bug.
    assert!(
        assigns("// nothing ever did work.stage = anything", "work.stage"),
        "if this stops being true, the comment filter in `production_lines` is the \
         only thing keeping prose out of the report — check it is still there"
    );
    assert!(production_lines().iter().all(|(_, _, l)| !l.trim_start().starts_with("//")));
}
