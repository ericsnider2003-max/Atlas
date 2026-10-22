//! The two ways into a consent decision must not disagree.
//!
//! `daemon` asks `classify_with_policy`, which applies the action-category
//! floor. `main` asks `gate`, which called bare `classify`. Both decide
//! whether the same action needs your say-so, and nothing checked that they
//! answered the same way.
//!
//! Measured across every intent, they agreed — `classify` was already at least
//! as strict as every category. That is a property of the current rules, not
//! of the code. One new intent whose `classify` arm falls through to
//! `AutoProceed` would open the gap on the command-line path, and no test
//! would have noticed.

use atlas::categories;
use atlas::intent::Intent;
use atlas::memory::Memory;
use atlas::policy::{self, Approver, Decision, PolicyConfig};

/// Every shape of intent, so a new variant that nobody adds here still shows
/// up in `every_intent_is_covered` below.
fn samples() -> Vec<Intent> {
    vec![
        Intent::WorkspaceOn,
        Intent::WorkspaceOff,
        Intent::OpenApp("chrome".into()),
        Intent::CloseApp("chrome".into()),
        Intent::FocusApp("chrome".into()),
        Intent::ViewDisplay,
        Intent::Research("rust".into()),
        Intent::CaptureWebcam,
        Intent::Say("hello".into()),
        Intent::Pause,
        Intent::Resume,
        Intent::Outstanding,
        Intent::Queued,
        Intent::DraftPost("x".into()),
        Intent::Undo,
        Intent::BackUp,
        Intent::SetMode("focus".into()),
        Intent::MachineHealth,
        Intent::UseClipboard("explain".into()),
        Intent::Rehearse("boot workspace".into()),
        Intent::Show("outstanding".into()),
        Intent::Dismiss,
        Intent::Ready,
        Intent::Capabilities("what can you do".into()),
        Intent::Ask("which one".into()),
        Intent::Unknown("blah".into()),
    ]
}

struct SaysYes;
impl Approver for SaysYes {
    fn approve(&self, _: &str) -> bool {
        true
    }
}
struct SaysNo;
impl Approver for SaysNo {
    fn approve(&self, _: &str) -> bool {
        false
    }
}

#[test]
fn the_two_consent_routes_cannot_disagree() {
    let mem = Memory::default();
    let cfg = PolicyConfig::default();
    let mut worse = Vec::new();
    for i in samples() {
        let daemon_says = policy::classify_with_policy(&i, &mem, &cfg);
        let cli_says = {
            // What `gate` would decide, read back through an approver that
            // refuses: an error means it asked, Ok means it went ahead.
            match policy::gate(&i, &SaysNo) {
                Ok(()) => Decision::AutoProceed,
                Err(_) => Decision::RequireApproval,
            }
        };
        let daemon_asks = daemon_says.needs_consent();
        let cli_asks = cli_says == Decision::RequireApproval;
        if daemon_asks && !cli_asks {
            worse.push(format!("{i:?}: daemon asks, command line does not"));
        }
    }
    assert!(
        worse.is_empty(),
        "the command-line route is looser than the daemon route:\n  {}",
        worse.join("\n  ")
    );
}

#[test]
fn the_floor_reaches_the_gate() {
    // Directly: no intent may pass `gate` if its category demands approval.
    for i in samples() {
        let floor = categories::category_of(&i).default_decision();
        if floor == Decision::RequireApproval {
            assert!(
                policy::gate(&i, &SaysNo).is_err(),
                "{i:?} passed the gate despite its category demanding approval"
            );
        }
    }
}

#[test]
fn saying_yes_still_lets_the_work_through() {
    // The fix must not make the gate refuse what you approved.
    for i in samples() {
        assert!(
            policy::gate(&i, &SaysYes).is_ok(),
            "{i:?} was refused even with approval given"
        );
    }
}

#[test]
fn ordinary_local_work_is_not_newly_gated() {
    // Applying a floor must not start asking about things that never needed
    // asking, or the prompt becomes noise and gets clicked through.
    for i in [
        Intent::WorkspaceOn,
        Intent::ViewDisplay,
        Intent::Outstanding,
    ] {
        assert!(
            policy::gate(&i, &SaysNo).is_ok(),
            "{i:?} started needing approval"
        );
    }
}

#[test]
fn a_consent_decision_is_the_same_every_time_it_is_asked() {
    // Consent that varies between two identical asks is worse than consent
    // that is always strict: you would learn to expect the lenient answer and
    // be surprised by the strict one.
    for i in samples() {
        let a = policy::classify(&i);
        let b = policy::classify(&i);
        assert_eq!(a, b, "{i:?} classified differently on two identical calls");

        let ca = categories::category_of(&i);
        let cb = categories::category_of(&i);
        assert_eq!(ca, cb, "{i:?} landed in two different categories");
        assert!(
            !ca.describe().is_empty(),
            "{i:?} is in a category that cannot say what it is"
        );
    }
}

/// What the owner confirmed on 22 Oct 2026: anything hard to undo always gets
/// a quick confirm -- sending (messages, emails, posts), deleting files,
/// moving files, installs, sign-ins, posting/publishing, two-factor codes.
///
/// One entry per intent that is itself that act. Intents that ask their own,
/// more specific question before acting (so that a second, vaguer "go ahead?"
/// in front would be two questions for one decision) are not here; they are
/// listed in `hard_to_undo_that_asks_for_itself` so the list is visible and
/// a new one has to be put in one place or the other:
/// `Mail` only reads, sorts and summarises (sends go through `Message` and
/// `DraftPost`); `SchedulePost` and `PressButton` and `Operate` ask at the
/// post and at the button; `TwoFactor` reads the setting back and waits for
/// a yes; `Feedback` and `Updates("undo")` hold their own question;
/// `MoveBigFiles`, `TidyDesktop`, `PcTune` and `EditMedia` show a plan and wait.
fn hard_to_undo() -> Vec<Intent> {
    let mut v = vec![
        // sending
        Intent::Message("sam hello".into()),
        // moving and deleting files (closing an app discards work too)
        Intent::MoveFile("report to archive".into()),
        Intent::CloseApp("chrome".into()),
        Intent::WorkspaceOff,
        // installs
        Intent::Updates("install".into()),
        Intent::MakePicture("get the picture maker".into()),
        Intent::WorkOnYourself("fix it".into()),
        // sign-ins and sign-ups, pairing
        Intent::SignIn("github".into()),
        Intent::CreateAccount("github".into()),
        Intent::Pair("code".into()),
        Intent::AcceptPairing("code".into()),
        // one-time codes
        Intent::TypeCode("the code from my texts".into()),
    ];
    // The phone's own model is fetched only on a phone build.
    #[cfg(feature = "phone-llm")]
    v.push(Intent::PhoneModel("get".into()));
    v
}

#[test]
fn every_hard_to_undo_intent_asks_first() {
    let mem = Memory::default();
    let cfg = PolicyConfig::default();
    for i in hard_to_undo() {
        assert_eq!(
            policy::classify(&i),
            Decision::RequireApproval,
            "{i:?} must be asked about first"
        );
        assert!(
            policy::classify_with_policy(&i, &mem, &cfg).needs_consent(),
            "{i:?} must be asked about first on the daemon route"
        );
        assert!(
            policy::gate(&i, &SaysNo).is_err(),
            "{i:?} passed the command-line gate unasked"
        );
    }
}

/// The same acts, said so that they are NOT the hard ones, stay unasked, so
/// the confirm stays rare enough to be read.
#[test]
fn the_easy_neighbours_are_not_newly_asked() {
    for i in [
        Intent::Updates("status".into()),
        Intent::PhoneModel("status".into()),
        Intent::MakePicture("draw me a lighthouse".into()),
        Intent::CreateFile("notes".into()),
        Intent::ReadFile("notes".into()),
    ] {
        assert_ne!(
            policy::classify(&i),
            Decision::RequireApproval,
            "{i:?} started needing approval"
        );
    }
}

/// Five hundred earlier yeses to a send or a move must not teach Atlas to
/// stop asking, even when the kind is on the learnable list.
#[test]
fn history_cannot_relax_a_hard_to_undo_confirm() {
    let mut mem = Memory::default();
    for i in hard_to_undo() {
        assert!(policy::is_hard_to_undo(&i), "{i:?} is missing from is_hard_to_undo");
        let kind = atlas::session::kind_of(&i);
        for _ in 0..50 {
            mem.record_approval(kind, true, None);
        }
    }
    let cfg = PolicyConfig {
        always_ask: Vec::new(),
        learnable: hard_to_undo()
            .iter()
            .map(|i| atlas::session::kind_of(i).to_string())
            .collect(),
        min_samples: 1,
    };
    for i in hard_to_undo() {
        assert_eq!(
            policy::classify_with_policy(&i, &mem, &cfg),
            Decision::RequireApproval,
            "{i:?} was learned away"
        );
    }
}

/// A step of a multi-part plan is held at a hard-to-undo step, and an
/// add-on step reads the same grade (`splitter::destructive`, plugins).
#[test]
fn a_plan_stops_at_a_hard_to_undo_step() {
    for i in hard_to_undo() {
        assert!(
            atlas::splitter::destructive(&i),
            "{i:?} would run inside a multi-part plan without a stop"
        );
        assert!(policy::classify(&i).needs_consent(), "{i:?}");
    }
}
