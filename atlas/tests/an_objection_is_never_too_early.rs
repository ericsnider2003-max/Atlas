//! Somebody said "please don't record me" and Atlas recorded the call.
//!
//! ## The window
//!
//! `someone_objected` began:
//!
//! ```ignore
//! self.objections.push(who.to_string());
//! if self.state != State::Recording { return Step::Nothing; }
//! ```
//!
//! The objection went onto the list and was then thrown away unless recording
//! had already started. `State::Announcing` is the window while the
//! announcement is being delivered — and that is **the moment people
//! actually object**, because they have just been told. An objection there
//! did nothing, and `announcement_delivered` then set
//! `announcement_landed = true`, `scope = Everyone`, `state = Recording`, and
//! returned `Step::Start(Everyone)`.
//!
//! `recorded_others_without_announcing()` — the method whose own doc calls it
//! *"the invariant the whole module exists to hold"* — returned `false`,
//! because the announcement had in fact landed. The invariant held and the
//! promise did not: announcing is what the module measures and consent is
//! what it is for.
//!
//! ## And the switch that wasn't checked
//!
//! `call_started` refuses when `cfg.enabled` is false. `you_approved` and
//! `you_declined` did not — they went straight to announcing or to recording
//! your side. The question and the answer are separated by a person thinking
//! about it, so the switch can be turned off in between; and a spoken "yes,
//! record it" reaching `you_approved` without `call_started` having run would
//! start a recording of a feature that is switched off.
//!
//! ## Status
//!
//! `consent` is on `tests/wiring.rs`'s declared `UNWIRED_BASELINE` — present,
//! tested, not yet wired to a caller, waiting on Eric's ruling because it
//! acts on his calls. So these were latent rather than live. They are fixed
//! now so that wiring it is a decision about whether to, not a decision that
//! also has to find these.

use atlas::consent::{Recorder, ConsentConfig, Scope, Step};

fn on() -> ConsentConfig {
    // The announce-and-object mode, which still exists beside asking.
    ConsentConfig { enabled: true, ask_every_call: false, ask_the_others: false, ..Default::default() }
}

/// Up to the moment the announcement is going out.
fn announcing() -> Recorder {
    let mut c = Recorder::new(on());
    match c.call_started(Scope::Everyone) {
        Step::Announce(_) => {}
        o => panic!("a call did not reach the announcement: {o:?}"),
    }
    c
}

#[test]
fn objecting_while_the_announcement_is_going_out_stops_it() {
    let mut c = announcing();

    let step = c.someone_objected("Priya");
    assert!(
        !matches!(step, Step::Nothing),
        "an objection during the announcement did nothing at all: {step:?}"
    );

    // And the announcement landing afterwards does not start it.
    let after = c.announcement_delivered();
    assert!(
        !matches!(after, Step::Start(Scope::Everyone)),
        "it started recording everyone after somebody objected: {after:?}"
    );
    assert!(
        !c.capturing_others(),
        "it is capturing everyone on a call where somebody asked it not to"
    );
}

#[test]
fn the_objection_and_the_announcement_landing_together_still_stops_it() {
    // The two race by their nature — the announcement is what people are
    // responding to. Checked from the other side: the objection arrives
    // between the announcement going out and the confirmation coming back.
    let mut c = announcing();
    c.someone_objected("Priya");
    let after = c.announcement_delivered();
    match after {
        Step::AskYou(said) => {
            assert!(said.contains("Priya"), "it did not say who objected: {said}");
            assert!(
                said.contains("your side"),
                "it did not say what it is doing instead: {said}"
            );
        }
        o => panic!("the announcement landed and it did not say an objection stopped it: {o:?}"),
    }
}

#[test]
fn the_invariant_check_is_not_what_was_holding_this_up() {
    // The reason nothing noticed. `recorded_others_without_announcing` asks
    // whether the announcement happened, and it did. So the module's own
    // self-check reported everything fine while it recorded somebody who had
    // said no — which is why this file checks `capturing_others` instead.
    let mut c = announcing();
    c.someone_objected("Priya");
    c.announcement_delivered();
    assert!(
        !c.recorded_others_without_announcing(),
        "the announcement genuinely did go out, so this stays false either way — it is \
         not the check that catches an objection"
    );
    assert!(!c.capturing_others(), "and this is the one that does");
}

#[test]
fn objecting_once_recording_has_started_stops_and_discards() {
    // The case that already worked, so the fix cannot be satisfied by
    // handling only the new one.
    let mut c = announcing();
    assert!(matches!(c.announcement_delivered(), Step::Start(Scope::Everyone)));
    assert!(c.capturing_others());

    match c.someone_objected("Priya") {
        Step::StopAndDiscard(said) => {
            assert!(said.contains("Priya"));
            assert!(said.contains("deleted"), "it kept what it had: {said}");
        }
        o => panic!("an objection mid-recording did not stop and discard: {o:?}"),
    }
    assert!(!c.capturing_others());
}

#[test]
fn an_objection_before_anything_starts_is_remembered_rather_than_acted_on() {
    // Nothing to stop, so nothing happens — but the objection is a fact about
    // this call and `call_started` is what clears the list for the next one.
    let mut c = Recorder::new(on());
    assert!(matches!(c.someone_objected("Priya"), Step::Nothing));

    // A fresh call is a fresh question.
    match c.call_started(Scope::Everyone) {
        Step::Announce(_) => {}
        o => panic!("a stale objection blocked the next call: {o:?}"),
    }
    assert!(matches!(c.announcement_delivered(), Step::Start(Scope::Everyone)));
}

#[test]
fn a_call_recording_nothing_but_you_is_unaffected_by_an_objection_to_it() {
    // Your own microphone captures only you. Nobody else's consent is
    // involved — so this must not become a way for a third party to stop you
    // taking your own notes.
    let mut c = Recorder::new(on());
    assert!(matches!(c.call_started(Scope::YouOnly), Step::Start(Scope::YouOnly)));
    let step = c.someone_objected("Priya");
    assert!(
        matches!(step, Step::StopAndDiscard(_)),
        "objecting to a your-side-only recording is still honoured: {step:?}"
    );
}

// ===================== the switch =====================================

#[test]
fn saying_yes_does_nothing_while_the_feature_is_switched_off() {
    // `call_started` checks `cfg.enabled`; these two did not. The question
    // and the answer are separated by a person thinking about it, and a
    // spoken "yes, record it" can reach `you_approved` without
    // `call_started` having run at all.
    let mut c = Recorder::new(ConsentConfig { enabled: false, ..on() });
    assert!(matches!(c.you_approved(), Step::Nothing), "it started announcing with the feature off");
    assert!(!c.capturing_others());
    assert!(c.indicator().is_none(), "it is showing a recording indicator with the feature off");
}

#[test]
fn saying_no_does_nothing_while_the_feature_is_switched_off() {
    // `you_declined` set `state = Recording` outright — so declining to
    // record everyone started recording your side, whatever the switch said.
    let mut c = Recorder::new(ConsentConfig { enabled: false, ..on() });
    assert!(matches!(c.you_declined(), Step::Nothing), "declining started a recording");
    assert!(c.indicator().is_none(), "it is recording your side with the feature off");
}

#[test]
fn the_switch_being_on_still_lets_both_answers_work() {
    // So neither gate is satisfied by refusing everything.
    let mut c = Recorder::new(ConsentConfig { enabled: true, ask_every_call: true, ..on() });
    assert!(matches!(c.call_started(Scope::Everyone), Step::AskYou(_)));
    assert!(matches!(c.you_approved(), Step::Announce(_)));

    let mut d = Recorder::new(ConsentConfig { enabled: true, ask_every_call: true, ..on() });
    assert!(matches!(d.call_started(Scope::Everyone), Step::AskYou(_)));
    assert!(matches!(d.you_declined(), Step::Start(Scope::YouOnly)));
    assert_eq!(d.indicator().as_deref(), Some("noting your side"));
}

#[test]
fn the_module_is_wired_and_these_fixes_are_live() {
    // This file fixed two defects inside `consent` while it was latent. Since
    // 24 Sep 2026 call notes run it for real, so both fixes now guard real
    // calls: it is off the unwired baseline, and the running Atlas reaches it.
    let wiring = std::fs::read_to_string("tests/wiring.rs").expect("tests/wiring.rs");
    assert!(!wiring.contains("\"consent\","), "consent is listed as unwired again");
    let daemon = std::fs::read_to_string("src/callnotes.rs").expect("src/callnotes.rs");
    assert_eq!(daemon.matches("recorder.someone_objected(").count(), 1, "an objection no longer reaches the recorder");
}
