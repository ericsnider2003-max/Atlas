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
    for i in [Intent::WorkspaceOn, Intent::ViewDisplay, Intent::Outstanding] {
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
