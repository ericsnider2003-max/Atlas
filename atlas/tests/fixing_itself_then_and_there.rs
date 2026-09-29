//! "If it passes, it gets fixed then and there" — the last four words.
//!
//! The rule: Atlas identifies faulty code, works out a fix in a sandbox, and
//! **if it passes, it is fixed then and there** — provided the fix does not
//! change or limit what Atlas can do. Something that is not a fault but a gap
//! is a different thing entirely: it gets scoped as a build plan and put to
//! you to decide on.
//!
//! Every piece of that existed. `pipeline` is the five stages and refuses to
//! land without all of them; `sandbox` creates a scratch tree, runs a tool in
//! it and plans what would come back; `selfwork::may_edit` knows which paths
//! are Atlas's and which decide what Atlas is allowed to do; `pipeline::review`
//! blocks a fix that landed somewhere other than where the cause was, or that
//! weakened a test.
//!
//! Two things were missing, and both were at the end:
//!
//! 1. **`Next::Land` did not land.** `sandbox::plan` calls itself "the
//!    preview" and it was the whole story — nothing ever applied a `Change`.
//!    The daemon printed the sentence describing what would change.
//! 2. **The session was rebuilt every turn.** `Session::new(what, 0)` on each
//!    `WorkOnYourself`, so `stage` was always `Thought` and `thought` always
//!    `None`. A state machine with no state answers with its first state
//!    forever — it could not reach Build, let alone Implement.
//!
//! And the constraint that comes with the rule — *does not change or limit
//! what Atlas can do* — is `mend::paper_overs`, which had no caller. A deleted
//! test, a silenced warning or a widened type removes the thing that would
//! have noticed. That **is** limiting what Atlas can do.

use atlas::pipeline::{Concern, Note, Review};
use atlas::sandbox::{Change, Fingerprint};
use atlas::selfwork::{land, what_holds_it_back, Held, SelfWorkConfig, Session};
use std::path::PathBuf;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-land-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cfg() -> SelfWorkConfig {
    let c = atlas::config::Config::load(std::path::Path::new("config")).unwrap();
    c.tools.as_ref().unwrap().self_work.clone()
}

/// A change from `sandbox` content to a real file.
fn change(dir: &PathBuf, target_rel: &str, sandbox_text: &str, target_text: Option<&str>) -> Change {
    let source = dir.join("sandbox-copy.rs");
    std::fs::write(&source, sandbox_text).unwrap();
    let target = dir.join(target_rel);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let new_file = target_text.is_none();
    if let Some(t) = target_text {
        std::fs::write(&target, t).unwrap();
    }
    Change {
        bytes: sandbox_text.len() as u64,
        new_file,
        target_was: Fingerprint::of(&target),
        target,
        source,
    }
}

fn clean_review() -> Review {
    Review { notes: vec![Note { kind: Concern::Fine, what: "does what it said".into() }] }
}

/// A standing grant that covers an ordinary fix.
///
/// Explicit rather than taken from `config/tools.yaml`, which ships
/// `may_change: nothing`. These tests are about the landing machinery, and
/// with nothing granted every one of them would be held back for the same
/// reason and would stop testing the thing it names.
/// `the_shipped_config_grants_nothing_so_nothing_lands_by_itself` is the test
/// for the shipped setting.
fn grant() -> atlas::selfgrant::SelfGrantConfig {
    atlas::selfgrant::SelfGrantConfig {
        may_change: "how_it_decides".into(),
        ..Default::default()
    }
}

// ============ a real fix lands =========================================

#[test]
fn a_change_that_passes_and_papers_over_nothing_lands() {
    let dir = tmp("lands");
    let c = change(
        &dir,
        "src/thing.rs",
        "fn thing() -> u32 {\n    compute()\n}\n",
        Some("fn thing() -> u32 {\n    0\n}\n"),
    );

    let held = what_holds_it_back(std::slice::from_ref(&c), Some(&clean_review()), &cfg(), &grant(), &dir);
    assert!(held.is_empty(), "a genuine fix was held back: {held:?}");

    let keep = dir.join("kept");
    assert_eq!(land(std::slice::from_ref(&c), &keep).unwrap(), 1);
    assert!(
        std::fs::read_to_string(&c.target).unwrap().contains("compute()"),
        "it reported landing and the file is unchanged"
    );
}

#[test]
fn a_land_that_fails_partway_undoes_everything_it_already_did() {
    // The safety an unattended land rests on: if the third file can't be
    // written, the first two must not be left changed. A half-landed fix on a
    // machine nobody is watching is the worst outcome, so land is all-or-nothing.
    let dir = tmp("rollback");

    // 1. Overwrite an existing file.
    let src1 = dir.join("src1.rs");
    std::fs::write(&src1, "the new version").unwrap();
    let tgt1 = dir.join("a.rs");
    std::fs::write(&tgt1, "the old version").unwrap();
    let c1 = Change { bytes: 15, new_file: false, target_was: Fingerprint::of(&tgt1), target: tgt1.clone(), source: src1 };

    // 2. Create a brand-new file.
    let src2 = dir.join("src2.rs");
    std::fs::write(&src2, "a new file's contents").unwrap();
    let tgt2 = dir.join("b.rs");
    let c2 = Change { bytes: 21, new_file: true, target_was: None, target: tgt2.clone(), source: src2 };

    // 3. A change whose source does not exist, so the copy fails.
    let tgt3 = dir.join("c.rs");
    let c3 = Change { bytes: 0, new_file: true, target_was: None, target: tgt3.clone(), source: dir.join("missing.rs") };

    let keep = dir.join("kept");
    let result = land(&[c1, c2, c3], &keep);
    assert!(result.is_err(), "a land with an unreadable source must fail, not partially succeed");
    assert_eq!(std::fs::read_to_string(&tgt1).unwrap(), "the old version", "the overwritten file was restored");
    assert!(!tgt2.exists(), "the new file created before the failure was removed");
    assert!(!tgt3.exists(), "the file that failed to copy is not on disk");
}

#[test]
fn the_version_it_replaced_is_kept_so_you_can_put_it_back() {
    // A fix Atlas landed on its own has to be something you can undo without
    // asking it.
    let dir = tmp("kept");
    let c = change(&dir, "src/thing.rs", "the new one\n", Some("the old one\n"));
    let keep = dir.join("kept");
    land(std::slice::from_ref(&c), &keep).unwrap();

    let back = std::fs::read_to_string(keep.join("thing.rs.before")).unwrap();
    assert_eq!(back, "the old one\n", "the previous version was not kept");
}

// ============ the constraint: it must not limit what Atlas can do ======

#[test]
fn a_change_that_deletes_the_test_is_held() {
    // The cheapest way past any failing check, and the one an assistant graded
    // on "make it green" finds every time.
    let dir = tmp("deleted");
    // The removed test is built rather than written as a literal.
    //
    // `tests/retrospective.rs` finds test bodies by splitting the file on the
    // test attribute, and a fixture containing one splits this body in half —
    // the first half then has no assertion in it and is reported as a test
    // that asserts nothing. The detector is not wrong; the fixture was code
    // masquerading as code. Assembled at runtime, it is data.
    //
    // The first fix put the attribute in *this comment* while explaining the
    // problem, and the guard caught it again. Naming it in prose is enough.
    let marker = format!("#[{}]", "test");
    let with_a_test = format!("fn thing() {{}}\n{marker}\nfn it_works() {{ assert!(thing()); }}\n");
    let c = change(&dir, "src/thing.rs", "fn thing() {}\n", Some(&with_a_test));

    let held = what_holds_it_back(std::slice::from_ref(&c), Some(&clean_review()), &cfg(), &grant(), &dir);
    assert!(
        held.iter().any(|h| matches!(h, Held::PapersOver(_))),
        "a deleted test was allowed to land: {held:?}"
    );
}

#[test]
fn the_six_paper_over_shapes_are_each_held() {
    // None of these changes the test count, so a review that only counts would
    // pass every one. `paper_overs` reads the shapes.
    for (name, line) in [
        ("a skipped test", "#[ignore]"),
        ("a silenced warning", "#[allow(dead_code)]"),
        ("a widened type", "fn thing() -> Any {}"),
        ("a swallowed error", "except Exception: pass"),
        ("a defaulted failure", "let n = compute().unwrap_or_default();"),
    ] {
        let dir = tmp(&format!("shape-{}", line.len()));
        let c = change(&dir, "src/thing.rs", &format!("fn thing() {{}}\n{line}\n"), Some("fn thing() {}\n"));
        let held = what_holds_it_back(std::slice::from_ref(&c), Some(&clean_review()), &cfg(), &grant(), &dir);
        assert!(
            held.iter().any(|h| matches!(h, Held::PapersOver(_))),
            "{name} was allowed to land: {held:?}"
        );
    }
}

#[test]
fn what_is_wrong_with_it_is_said_rather_than_just_refused() {
    let dir = tmp("why");
    let c = change(&dir, "src/thing.rs", "fn thing() {}\n#[ignore]\n", Some("fn thing() {}\n"));
    let held = what_holds_it_back(std::slice::from_ref(&c), Some(&clean_review()), &cfg(), &grant(), &dir);
    let said = held
        .iter()
        .find(|h| matches!(h, Held::PapersOver(_)))
        .expect("no paper-over was reported")
        .plain();
    assert!(said.contains("never fail again"), "it did not say why it is not a fix: {said}");
    assert!(said.contains("haven't landed it"), "it did not say what it did: {said}");
}

// ============ the paths that are not Atlas's ===========================

#[test]
fn it_may_not_edit_the_file_that_decides_what_it_may_edit() {
    // A system that can edit its own permissions has none.
    let dir = tmp("permissions");
    let c = change(&dir, "src/policy.rs", "fn anything_goes() {}\n", Some("fn strict() {}\n"));
    let held = what_holds_it_back(std::slice::from_ref(&c), Some(&clean_review()), &cfg(), &grant(), &dir);
    assert!(
        held.iter().any(|h| matches!(h, Held::NotMine(_))),
        "it was allowed to rewrite its own limits: {held:?}"
    );
}

#[test]
fn a_path_outside_the_parts_it_works_on_is_held() {
    let dir = tmp("outside");
    let c = change(&dir, "elsewhere/thing.rs", "new\n", Some("old\n"));
    let held = what_holds_it_back(std::slice::from_ref(&c), Some(&clean_review()), &cfg(), &grant(), &dir);
    assert!(held.iter().any(|h| matches!(h, Held::NotMine(_))), "got: {held:?}");
}

// ============ your edits win =============================================

#[test]
fn it_does_not_land_on_top_of_something_you_changed_meanwhile() {
    // `Change::target_was` was written for exactly this: overnight work makes
    // the gap hours wide, and a whole-file copy silently wins against an
    // evening's editing.
    let dir = tmp("yours");
    let c = change(&dir, "src/thing.rs", "what Atlas built\n", Some("what was there\n"));

    // You edit it while Atlas is working.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(&c.target, "what you just wrote, which is longer than before\n").unwrap();

    let held = what_holds_it_back(std::slice::from_ref(&c), Some(&clean_review()), &cfg(), &grant(), &dir);
    assert!(
        held.iter().any(|h| matches!(h, Held::YouChangedIt(_))),
        "it landed over your edit: {held:?}"
    );
    let said = held
        .iter()
        .find(|h| matches!(h, Held::YouChangedIt(_)))
        .expect("your edit was not the reason given")
        .plain();
    assert!(said.contains("still in the sandbox"), "it did not say where the work went: {said}");
}

#[test]
fn a_file_you_deleted_is_not_quietly_recreated() {
    let dir = tmp("deleted-by-you");
    let c = change(&dir, "src/thing.rs", "new\n", Some("old\n"));
    std::fs::remove_file(&c.target).unwrap();

    let held = what_holds_it_back(std::slice::from_ref(&c), Some(&clean_review()), &cfg(), &grant(), &dir);
    assert!(
        held.iter().any(|h| matches!(h, Held::YouChangedIt(_))),
        "it put back a file you deleted: {held:?}"
    );
}

#[test]
fn a_genuinely_new_file_is_not_treated_as_one_you_changed() {
    let dir = tmp("new");
    let c = change(&dir, "src/brand_new.rs", "fn fresh() {}\n", None);
    let held =
        what_holds_it_back(std::slice::from_ref(&c), Some(&clean_review()), &cfg(), &grant(), &dir);
    // Narrowed from `held.is_empty()`, which was asserting more than this
    // test is about. A new file really is held — as `Reach::SomethingNew`,
    // which `how_it_decides` does not cover and which
    // `a_new_module_is_asked_about_rather_than_added_quietly` below pins. The
    // check *this* test names is the fingerprint one: a file that was never
    // there cannot have been changed under Atlas while it worked.
    assert!(
        !held.iter().any(|h| matches!(h, Held::YouChangedIt(_))),
        "a file that did not exist was reported as one you edited meanwhile: {held:?}"
    );
    assert!(
        !held.iter().any(|h| matches!(h, Held::NotMine(_) | Held::PapersOver(_))),
        "a plain new source file was refused on its path or its contents: {held:?}"
    );
}

#[test]
fn a_new_module_is_asked_about_rather_than_added_quietly() {
    // `Reach::SomethingNew` sits above `HowItDecides` on purpose: additive
    // and reversible, but a new thing in the system rather than a change to
    // an existing one, "which is a different decision and deserves to be
    // asked as one". Nothing consulted that, because `may_land` had no
    // caller — so a new module landed under a grant that covers rewording.
    let dir = tmp("new-module");
    let c = change(&dir, "src/brand_new.rs", "fn fresh() {}\n", None);
    let held =
        what_holds_it_back(std::slice::from_ref(&c), Some(&clean_review()), &cfg(), &grant(), &dir);
    let why = held
        .iter()
        .find_map(|h| match h {
            Held::NotGranted(w) => Some(w.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("a whole new module was added without asking: {held:?}"));
    assert!(
        why.contains("adding something new"),
        "the reason doesn't say it's a new thing rather than a change: {why}"
    );
}

// ============ the review still blocks ===================================

#[test]
fn a_fix_the_review_blocked_does_not_land() {
    let dir = tmp("review");
    let c = change(&dir, "src/thing.rs", "fixed\n", Some("broken\n"));
    let blocked = Review {
        notes: vec![Note {
            kind: Concern::NotWhereTheCauseWas,
            what: "the cause was in src/other.rs and nothing there changed".into(),
        }],
    };
    let held = what_holds_it_back(std::slice::from_ref(&c), Some(&blocked), &cfg(), &grant(), &dir);
    assert!(held.iter().any(|h| matches!(h, Held::ReviewSaysNo(_))), "got: {held:?}");
}

#[test]
fn every_reason_is_reported_not_only_the_first() {
    // Three problems learned one round at a time is three rounds, and the
    // second and third are usually the informative ones.
    let dir = tmp("all");
    let c = change(&dir, "src/policy.rs", "fn thing() {}\n#[ignore]\n", Some("fn thing() {}\n"));
    let blocked = Review {
        notes: vec![Note { kind: Concern::TestWeakened, what: "two fewer tests".into() }],
    };
    let held = what_holds_it_back(std::slice::from_ref(&c), Some(&blocked), &cfg(), &grant(), &dir);

    assert!(held.len() >= 3, "only {} reason(s) reported: {held:?}", held.len());
    assert!(held.iter().any(|h| matches!(h, Held::NotMine(_))));
    assert!(held.iter().any(|h| matches!(h, Held::PapersOver(_))));
    assert!(held.iter().any(|h| matches!(h, Held::ReviewSaysNo(_))));
}

// ============ how far you said it may go ================================
//
// `selfgrant::may_land` is the function that knows what a standing grant
// covers, and it had **no production caller**. `may_edit` is binary, so a
// change to `src/browser.rs` landed exactly as readily as one to
// `src/persona.rs`, and the shipped `tools.yaml` says `may_change: nothing`.

#[test]
fn the_shipped_config_grants_nothing_so_nothing_lands_by_itself() {
    // The setting Atlas actually ships with. A change that passes every other
    // check is still held, and the reason says it is yours rather than
    // blaming the change.
    let shipped = atlas::config::Config::load(std::path::Path::new("config"))
        .unwrap()
        .tools
        .as_ref()
        .unwrap()
        .self_grant
        .clone();
    assert!(
        shipped.granted().is_none(),
        "config/tools.yaml now grants {:?} — this test is about the shipped default",
        shipped.may_change
    );

    let dir = tmp("ungranted");
    let c = change(&dir, "src/thing.rs", "fixed\n", Some("broken\n"));
    let held = what_holds_it_back(
        std::slice::from_ref(&c),
        Some(&clean_review()),
        &cfg(),
        &shipped,
        &dir,
    );
    let ask = held
        .iter()
        .find(|h| matches!(h, Held::NotGranted(_)))
        .unwrap_or_else(|| panic!("it landed a change on its own with nothing granted: {held:?}"));
    let said = ask.plain();
    assert!(
        said.contains("yours rather than mine"),
        "it didn't say whose decision it is: {said}"
    );
}

#[test]
fn a_change_reaching_further_than_the_grant_is_held_and_says_so() {
    // `src/browser.rs` is `Reach::WhatItTouches`; the grant here is
    // `how_it_decides`. It is a real file Atlas is allowed to edit, so this
    // is the grant refusing rather than the never-list.
    let dir = tmp("further");
    let c = change(&dir, "src/browser.rs", "fn new() {}\n", Some("fn old() {}\n"));
    let held =
        what_holds_it_back(std::slice::from_ref(&c), Some(&clean_review()), &cfg(), &grant(), &dir);
    assert!(
        !held.iter().any(|h| matches!(h, Held::NotMine(_))),
        "src/browser.rs is not on the never-list, so this is the wrong refusal: {held:?}"
    );
    let why = held
        .iter()
        .find_map(|h| match h {
            Held::NotGranted(w) => Some(w.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("it landed past what was granted: {held:?}"));
    assert!(
        why.contains("what it does to your machine") && why.contains("how it decides"),
        "the reason names neither how far it reaches nor how far you allowed: {why}"
    );
}

#[test]
fn a_change_to_its_own_limits_is_never_rather_than_not_yet() {
    // The distinction matters: "you haven't said I can" invites you to say
    // it, and this is not a thing you can say.
    let dir = tmp("never");
    let c = change(&dir, "config/tools.yaml", "self_grant:\n  may_change: its_own_limits\n",
        Some("self_grant:\n  may_change: nothing\n"));
    let held =
        what_holds_it_back(std::slice::from_ref(&c), Some(&clean_review()), &cfg(), &grant(), &dir);
    assert!(
        held.iter().any(|h| matches!(h, Held::NeverMine(_))),
        "editing the file that holds its own grant was treated as merely ungranted: {held:?}"
    );
    // And `may_edit` refuses it independently, so the two gates agree.
    assert!(
        held.iter().any(|h| matches!(h, Held::NotMine(_))),
        "`may_edit` still allows config/tools.yaml, which is where `self_grant:` lives: {held:?}"
    );
}

// ============ the state machine has state ===============================

#[test]
fn a_piece_of_work_survives_the_turn_it_started_in() {
    // `Session::new` every turn meant `stage` was always Thought and `thought`
    // always None: it answered "Thought next." forever and could never reach
    // Build, let alone Implement.
    let s = Session::new("fix the thing", 0);
    let json = serde_json::to_string(&s).expect("a session that cannot be saved cannot be resumed");
    let back: Session = serde_json::from_str(&json).unwrap();
    assert_eq!(back.goal, "fix the thing");
    assert_eq!(back.work.stage, atlas::pipeline::Stage::Thought);
}

#[test]
fn a_session_carries_its_stage_across_a_save() {
    let mut s = Session::new("fix the thing", 0);
    s.work.stage = atlas::pipeline::Stage::Review;
    s.work.rounds = 2;
    let back: Session = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back.work.stage, atlas::pipeline::Stage::Review, "the stage was lost");
    assert_eq!(back.work.rounds, 2, "the rounds were lost, so max_rounds can never bite");
}
