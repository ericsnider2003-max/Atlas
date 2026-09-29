//! "That needs your say-so. I'll wait." — and nothing waited.
//!
//! `mend.rs`'s second half is about asking someone who does not read code: a
//! `Question` that knows how to be phrased, how to check it is answerable
//! without jargon, how to read out loud, and what backlog item it becomes.
//!
//! **Nothing ever built one.** So `Blocker::NeedsYourDecision` — the one
//! backlog variant meaning "Atlas found something only you can settle" — was
//! produced by exactly one function, `Question::parked()`, and that function
//! had no caller. The variant existed, `explain()` described it, the brief
//! surfaces it, `self_clearing()` knew it does not clear on its own, and
//! nothing in the program could ever create one.
//!
//! What was missing was not a caller. It was noticing the daemon already had
//! the moment this is for and dropped it:
//!
//! ```ignore
//! Decision::RequireApproval => {
//!     if self.autonomy == Autonomy::Unattended {
//!         "That needs your say-so. I'll wait.".into()
//!     }
//! ```
//!
//! Nothing waited. No record was kept, so the next time you looked there was
//! no trace you had been asked. A promise Atlas does not keep is worse than a
//! refusal — and `daemon::called` had been sitting there since the same night
//! with the note "not yet called from anywhere — `mend`'s question-parking
//! flow is not wired into the daemon loop tonight".

use atlas::backlog::Blocker;
use atlas::config::Config;
use atlas::daemon::{Autonomy, Daemon};
use atlas::mend::{about_approval, should_ask, Kind, Question};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

const NOW: u64 = 1_700_000_000;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-park-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}

fn away<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    let mut d = Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()));
    d.autonomy = Autonomy::Unattended;
    d
}

// ============ the question is phrased for a person =======================

#[test]
fn an_approval_question_is_answerable_without_reading_code() {
    let q = about_approval("delete the old backups");
    assert!(q.answerable_without_code().is_ok(), "got jargon: {:?}", q.answerable_without_code());
    assert!(q.is_a_real_choice(), "two options, not an announcement and not a design exercise");
}

#[test]
fn a_real_turn_while_you_are_away_parks_rather_than_promising_to_wait() {
    // The end-to-end version, and the one that matters. An earlier draft of
    // this file called `park_for_you` directly, so reverting the daemon branch
    // to the old "I'll wait" left every test green — the same shape as the
    // brief test that passed on the backlog summary beside it.
    let (c, p) = (cfg(), plat());
    let mut d = away(&c, &p, "realturn");

    // Something graded `RequireApproval`: consequential, not reversible.
    // `close_app` is, and it takes an argument, so the parked question can
    // name what was asked for.
    let said = d.turn("close notepad", NOW);

    assert!(
        !said.contains("I'll wait"),
        "the old promise is back, and nothing waits: {said}"
    );
    assert_eq!(
        d.backlog.outstanding().len(),
        1,
        "a turn that needed your say-so left no trace: {said}"
    );
    assert!(matches!(d.backlog.outstanding()[0].blocker, Blocker::NeedsYourDecision { .. }));
}

#[test]
fn a_request_full_of_paths_is_still_phrased_plainly() {
    // The commonest reason Atlas cannot phrase a question is that it is
    // quoting your words back with a file path in them.
    // Asserted on the sentence itself, not on `answerable_without_code`.
    //
    // Going through the check was asserting a proxy, and it hid a real gap:
    // `NEEDS_CODE` catches `.rs`, `()` and `::`, but a bare path with no
    // extension — `/home/eric/old/report` — contains none of those, so the
    // check passes on a question with a filesystem path sitting in it. The
    // phrasing has to strip it whether or not the check would have noticed.
    for (request, shape) in [
        ("delete /home/eric/old/report and start again", "a path"),
        ("delete report.rs and start again", "an extension"),
        ("delete the thing and run cleanup() after", "a call"),
        ("delete the thing in atlas::daemon after", "a module path"),
        // Two technical words next to each other — the case that produces
        // "that that" if the run is not collapsed.
        ("delete /home/eric/old cleanup() now", "two in a row"),
    ] {
        let q = about_approval(request);
        let whole = format!("{} {} {}", q.doing, q.asks, q.set_aside);
        for bad in ['/', '('] {
            assert!(
                !whole.contains(bad),
                "{shape}: `{bad}` survived into the question — {}",
                q.asks
            );
        }
        assert!(!whole.contains("::"), "{shape}: a module path survived — {}", q.asks);
        assert!(!whole.contains(".rs"), "{shape}: an extension survived — {}", q.asks);
        // Two stripped words in a row must not read as "that that".
        assert!(!whole.contains("that that"), "{shape}: got `that that` — {}", q.asks);
        assert!(q.answerable_without_code().is_ok(), "{shape}: {:?}", q.answerable_without_code());
    }
}

#[test]
fn a_question_it_cannot_phrase_is_refused_rather_than_parked() {
    // The rule the whole module turns on: if Atlas cannot put it plainly it
    // has not understood it well enough to ask, and burying that in a backlog
    // item would hide the real problem.
    let (c, p) = (cfg(), plat());
    let mut d = away(&c, &p, "jargon");
    let q = Question {
        doing: "Running the thing.".into(),
        asks: "Should the parameter be null or should the function throw an exception?".into(),
        options: vec!["null".into(), "throw".into()],
        set_aside: "the thing".into(),
    };
    let said = d.park_for_you(&q, "run the thing", NOW);

    assert!(said.contains("can't put it in plain enough terms"), "got: {said}");
    assert!(d.backlog.outstanding().is_empty(), "an unanswerable question was parked anyway");
}

#[test]
fn one_option_is_an_announcement_and_is_not_parked_as_a_choice() {
    let (c, p) = (cfg(), plat());
    let mut d = away(&c, &p, "oneopt");
    let q = Question {
        doing: "Tidying up.".into(),
        asks: "Shall I tidy up?".into(),
        options: vec!["yes".into()],
        set_aside: "the tidying".into(),
    };
    assert!(!q.is_a_real_choice());
    let said = d.park_for_you(&q, "tidy up", NOW);
    assert!(said.contains("needs your say-so"), "got: {said}");
    assert!(d.backlog.outstanding().is_empty());
}

#[test]
fn nothing_is_guessed_while_it_waits() {
    // No default action, deliberately: a design decision guessed at is a
    // confident wrong answer with everything else built on top of it. What
    // the question promises is that one thing is set aside and the rest
    // carries on.
    let q = about_approval("delete the old backups");
    assert!(q.set_aside.contains("delete"), "it did not say what stops: {}", q.set_aside);
    let spoken = q.spoken("Eric");
    assert!(spoken.contains("carry on with the rest"), "got: {spoken}");
    assert!(spoken.starts_with("Eric,"), "it did not address you: {spoken}");
}

// ============ it survives you not being there ============================

#[test]
fn being_asked_while_you_are_away_leaves_something_behind() {
    // The whole point. Before this the branch returned a sentence and kept
    // nothing, so the next time you looked there was no trace.
    let (c, p) = (cfg(), plat());
    let mut d = away(&c, &p, "survives");
    let q = about_approval("delete the old backups");

    d.park_for_you(&q, "delete the old backups", NOW);

    let out = d.backlog.outstanding();
    assert_eq!(out.len(), 1, "nothing was left behind");
    assert!(
        matches!(out[0].blocker, Blocker::NeedsYourDecision { .. }),
        "parked under the wrong blocker: {:?}",
        out[0].blocker
    );
}

#[test]
fn it_does_not_clear_just_because_you_came_back() {
    // Being at the machine is not a decision. `self_clearing` already knew
    // that and had nothing to know it about.
    let q = about_approval("delete the old backups");
    assert!(
        !matches!(q.parked().self_clearing(), true),
        "a decision only you can make was treated as clearing itself"
    );
}

#[test]
fn the_parked_question_reaches_the_morning_brief() {
    // `Blocker::NeedsYourDecision` leads with what is waiting rather than with
    // the question, so a list of these reads as a list of work. The brief
    // surfaces it as something needing you.
    let (c, p) = (cfg(), plat());
    let mut d = away(&c, &p, "brief");
    d.park_for_you(&about_approval("delete the old backups"), "delete the old backups", NOW);

    let b = d.brief_now(NOW + 10);
    assert!(!b.is_empty(), "a parked decision did not reach the brief");
    assert!(
        b.yours.iter().any(|i| i.subject.contains("backups")),
        "the brief did not name it: {:?}",
        b.yours
    );
}

#[test]
fn it_survives_a_restart() {
    let store = tmp("restart");
    let (c, p) = (cfg(), plat());
    {
        let mut d = Daemon::new(
            &c, &p, None, Store::new(store.clone()), Proactive::new(ProactiveConfig::default()));
        d.autonomy = Autonomy::Unattended;
        d.park_for_you(&about_approval("delete the old backups"), "delete the old backups", NOW);
    }
    let d2 = Daemon::new(
        &c, &p, None, Store::new(store), Proactive::new(ProactiveConfig::default()));
    assert_eq!(d2.backlog.outstanding().len(), 1, "the question died with the process");
}

// ============ which failures are worth asking about ======================

#[test]
fn most_failures_are_never_put_to_you() {
    // Asking about every mechanical error is how an assistant becomes
    // something you stop reading.
    assert!(!should_ask(Kind::Mechanical, 9));
    assert!(!should_ask(Kind::Behavioural, 9));
    assert!(should_ask(Kind::Undecided, 0), "a decision only you can make waits for nobody");
    assert!(should_ask(Kind::Environment, 0));
}

#[test]
fn an_unclear_failure_gets_a_couple_of_goes_first() {
    assert!(!should_ask(Kind::Unclear, 0));
    assert!(!should_ask(Kind::Unclear, 1));
    assert!(should_ask(Kind::Unclear, 2), "plenty become clear once something has been tried");
}

// `about_ambiguity` and its test were deleted together. The moment is real --
// a clarification asked into an empty room is lost the same way an approval
// was -- but the daemon has one clarification sentence, not two readings, so
// building the question would have meant inventing the options. See the note
// in `src/mend.rs` and the entry in the outstanding list.
