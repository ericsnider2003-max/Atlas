//! Voice-lock, actually reachable.
//!
//! `voiceid.rs` was complete, careful, and had never run. Its own doc comment
//! says embeddings come from "an external speaker-encoder" — there was no
//! encoder anywhere in the tree, no tool configured for one, and
//! `VoiceId::check` had no caller. The module was reachable, tested, and did
//! nothing.
//!
//! It is worth being exact about how the wiring guard missed it, because this
//! is the sharpest example of that blind spot in the codebase: `voiceid` left
//! `UNWIRED_BASELINE` when `recall.rs` began calling `voiceid::cosine` to
//! compare *notes*. One vector-maths helper, borrowed for an unrelated job,
//! made the whole module count as wired while speaker identification still had
//! no path to it.
//!
//! So these tests are deliberately about the path, not the maths. The maths
//! already had tests while the feature did nothing.

use atlas::speaker::{available, parse_embedding, SpeakerConfig, NO_ENCODER};
use atlas::voiceid::{handle, Handling, Verdict, VoiceId, VoiceIdConfig};

fn on() -> VoiceIdConfig {
    VoiceIdConfig { enabled: true, ..Default::default() }
}

// --- the policy, which is the part that matters -----------------------------

#[test]
fn a_voice_that_is_not_yours_cannot_make_atlas_go_quiet() {
    // This test previously asserted the lockout as though it were the
    // feature: `NotYou` returned `Ignore` and the daemon broke out of the
    // turn, so a cold, a new headset, or sitting further from the microphone
    // read as Atlas being switched off -- no answer, no error, nothing said.
    //
    // Nothing distinguishes that from a podcast. `check` compares an
    // embedding to a centroid and returns a number; which side of the
    // threshold your own voice lands on is a matter of your throat and your
    // hardware. So the reading may escalate and may not subtract.
    assert_eq!(handle(Verdict::NotYou(0.1), false, &on()), Handling::Proceed);
    assert_eq!(handle(Verdict::NotYou(0.1), true, &on()), Handling::Confirm);
}

#[test]
fn no_verdict_of_any_kind_can_stop_a_harmless_thing_being_answered() {
    // The property, rather than one case of it: across every verdict the
    // encoder can produce, at every score, something harmless is answered.
    // A voice-lock that can silently swallow "what time is it" is a voice
    // lock that can silently swallow everything.
    for v in [
        Verdict::You(0.99),
        Verdict::Unsure(0.6),
        Verdict::NotYou(0.0),
        Verdict::NotYou(0.49),
        Verdict::NotEnrolled,
    ] {
        assert_eq!(
            handle(v, false, &on()),
            Handling::Proceed,
            "a harmless action was not answered on {v:?}"
        );
    }
}

#[test]
fn a_voice_reading_can_only_ever_escalate() {
    // Stated as the rule rather than as a list of cases, so a new verdict
    // added later is covered by it. `Handling` has no variant that grants and
    // none that refuses -- and that is enforced here rather than trusted,
    // because the refusing variant is exactly what was removed.
    for v in [Verdict::You(0.99), Verdict::Unsure(0.6), Verdict::NotYou(0.0), Verdict::NotEnrolled] {
        for consequential in [true, false] {
            let h = handle(v, consequential, &on());
            assert!(
                matches!(h, Handling::Proceed | Handling::Confirm),
                "a voice reading produced something other than proceed-or-ask: {h:?}"
            );
            if !consequential {
                assert_eq!(h, Handling::Proceed, "a harmless action was gated on {v:?}");
            }
        }
    }
}

#[test]
fn sounding_like_you_is_never_treated_as_proof() {
    // A recording of you sounds like you. Voice decides whether Atlas listens,
    // never whether Atlas is allowed.
    assert_eq!(handle(Verdict::You(0.99), true, &on()), Handling::Confirm);
}

#[test]
fn an_unsure_verdict_asks_rather_than_guessing_either_way() {
    // The instinct `recall::Clarity` already uses elsewhere: say so rather
    // than silently picking a side.
    assert_eq!(handle(Verdict::Unsure(0.6), true, &on()), Handling::Confirm);
    // And on something harmless it does not nag.
    assert_eq!(handle(Verdict::Unsure(0.6), false, &on()), Handling::Proceed);
}

#[test]
fn a_machine_that_was_never_taught_your_voice_does_not_gate_anything() {
    // The failure that would make this unusable: an unenrolled Atlas that
    // ignores everyone. Voice-lock unavailable must never behave as if
    // voice-lock were on.
    for consequential in [true, false] {
        assert_eq!(handle(Verdict::NotEnrolled, consequential, &on()), Handling::Proceed);
    }
}

#[test]
fn switching_it_off_switches_it_off() {
    let off = VoiceIdConfig { enabled: false, ..Default::default() };
    assert_eq!(handle(Verdict::NotYou(0.0), true, &off), Handling::Proceed);
}

// --- the encoder seam -------------------------------------------------------

#[test]
fn an_embedding_is_read_from_whatever_the_encoder_printed() {
    // Every speaker encoder prints something slightly different, and demanding
    // one exact format means the first one you try fails for a reason that has
    // nothing to do with your voice.
    let json: String = format!("[{}]", (0..32).map(|i| format!("{}.5", i)).collect::<Vec<_>>().join(","));
    let plain: String = (0..32).map(|i| format!("{}.5", i)).collect::<Vec<_>>().join(" ");
    let a = parse_embedding(&json).expect("json array");
    let b = parse_embedding(&plain).expect("plain numbers");
    assert_eq!(a, b, "the same numbers read differently depending on punctuation");
    assert_eq!(a.len(), 32);
}

#[test]
fn a_nan_is_refused_rather_than_poisoning_the_voiceprint() {
    // A NaN in the centroid makes every later comparison return 0, which reads
    // as "not you" forever — Atlas would silently stop listening to you and
    // the reason would be unrecoverable from the outside.
    let bad: String = format!("NaN {}", (0..32).map(|_| "0.5").collect::<Vec<_>>().join(" "));
    assert!(parse_embedding(&bad).is_err(), "a NaN was accepted into a voiceprint");
}

#[test]
fn something_that_is_not_numbers_is_refused_with_what_it_saw() {
    let e = parse_embedding("Traceback (most recent call last):").unwrap_err();
    let msg = format!("{e}");
    assert!(msg.contains("isn't a number"), "got: {msg}");
    assert!(msg.contains("Traceback"), "it didn't show what it choked on: {msg}");
}

#[test]
fn too_few_numbers_is_not_a_voice() {
    // A one-line error message parsed as three floats would otherwise become a
    // voiceprint that matches nothing.
    assert!(parse_embedding("0.1 0.2 0.3").is_err());
}

#[test]
fn no_encoder_configured_is_reported_as_unavailable_not_as_working() {
    let none = SpeakerConfig::default();
    assert!(!available(&none, &Default::default()));
    assert!(
        NO_ENCODER.contains("listen to whoever speaks"),
        "it doesn't say what actually happens instead"
    );
    assert!(
        NO_ENCODER.contains("isn't protecting you"),
        "an unavailable voice-lock could be read as an active one"
    );
}

// --- enrollment -------------------------------------------------------------

#[test]
fn enrolling_takes_several_samples_before_it_will_judge_anything() {
    // One recording captures one mood, one distance and one microphone.
    let cfg = on();
    let mut id = VoiceId::default();
    let a: Vec<f32> = (0..64).map(|i| (i as f32).sin()).collect();
    id.enroll(&a).unwrap();
    assert_eq!(
        id.check(&a, &cfg),
        Verdict::NotEnrolled,
        "it judged a voice off a single sample"
    );
    id.enroll(&a).unwrap();
    id.enroll(&a).unwrap();
    assert!(matches!(id.check(&a, &cfg), Verdict::You(_)), "three samples should be enough");
}

#[test]
fn a_different_voice_does_not_match() {
    let cfg = on();
    let mut id = VoiceId::default();
    let me: Vec<f32> = (0..64).map(|i| (i as f32 * 0.1).sin()).collect();
    for _ in 0..3 {
        id.enroll(&me).unwrap();
    }
    let someone_else: Vec<f32> = (0..64).map(|i| -(i as f32 * 0.1).sin()).collect();
    assert!(
        matches!(id.check(&someone_else, &cfg), Verdict::NotYou(_)),
        "an opposite embedding was accepted as you"
    );
}

#[test]
fn changing_encoder_is_refused_rather_than_silently_compared() {
    // Embeddings from two different encoders are not comparable, and comparing
    // them anyway produces a confident wrong answer.
    let mut id = VoiceId::default();
    id.enroll(&vec![0.5f32; 64]).unwrap();
    assert!(id.enroll(&vec![0.5f32; 128]).is_err(), "mismatched sizes were accepted");
}

#[test]
fn an_embedding_of_a_different_size_reads_as_not_enrolled() {
    // Rather than as a failed match, which would look like someone else
    // talking when in fact the encoder changed.
    let cfg = on();
    let mut id = VoiceId::default();
    for _ in 0..3 {
        id.enroll(&vec![0.5f32; 64]).unwrap();
    }
    assert_eq!(id.check(&vec![0.5f32; 128], &cfg), Verdict::NotEnrolled);
}

// --- the wiring itself ------------------------------------------------------

#[test]
fn the_daemon_holds_a_verdict_and_starts_with_no_opinion() {
    // A typed line has no voice to judge, and the honest default for that is
    // "not enrolled" — which proceeds — rather than anything that could gate.
    use atlas::config::Config;
    use atlas::daemon::Daemon;
    use atlas::platform::mock::MockPlatform;
    use atlas::platform::Monitor;
    use atlas::proactive::{Proactive, ProactiveConfig};
    use atlas::store::Store;

    let c = Config::load(std::path::Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }]);
    let dir = std::env::temp_dir().join("atlas-vl-default");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let d = Daemon::new(&c, &p, None, Store::new(dir), Proactive::new(ProactiveConfig::default()));
    assert_eq!(d.last_verdict, Verdict::NotEnrolled);
}

#[test]
fn ears_that_cannot_hear_a_voiceprint_say_so_rather_than_returning_one() {
    // The default on the trait. An implementation that cannot tell voices
    // apart must not have that mistaken for a passing check.
    struct Deaf;
    impl atlas::daemon::Ears for Deaf {
        fn wait_for_wake(&self) -> atlas::error::Result<()> {
            Ok(())
        }
        fn listen(&self) -> atlas::error::Result<String> {
            Ok("hello".into())
        }
        fn listen_briefly(&self, _: u32) -> atlas::error::Result<Option<String>> {
            Ok(None)
        }
    }
    assert!(atlas::daemon::Ears::voiceprint(&Deaf).is_none());
}

#[test]
fn the_shipped_config_leaves_voice_lock_off_until_it_can_work() {
    // Shipping it on with no encoder would mean every verdict is NotEnrolled
    // and the setting is a lie in the direction of safety, which is still a lie.
    let c = atlas::config::Config::load(std::path::Path::new("config")).unwrap();
    let t = c.tools.as_ref().expect("tools.yaml loads");
    assert!(!t.voice_id.enabled, "voice-lock ships on without an encoder to back it");
    assert!(t.voice_id.min_samples >= 2, "one sample is not an enrollment");
    assert!(
        t.voice_id.reject < t.voice_id.accept,
        "the grey band is inverted, so every verdict is decided"
    );
}

// --- measuring the thresholds, without moving them --------------------------
//
// The adaptive-threshold question is an open ruling: a threshold that moves
// toward accepted samples accepts more over time, which is right for
// ergonomics and wrong for a credential. What CAN be built short of it is
// the evidence — where the fixed lines sit against this voice's own scores —
// and these tests hold that the evidence is gathered, said, and never acts.

#[test]
fn accepted_scores_are_kept_bounded_and_never_move_the_thresholds() {
    let cfg = on();
    let mut id = VoiceId::default();
    for i in 0..100 {
        id.note_accepted(0.80 + (i % 5) as f32 * 0.01);
    }
    assert!(id.accepted.len() <= 60, "the score history is a distribution, not a diary");
    // The config is untouched by anything voiceid does with the history.
    assert_eq!(cfg.accept, VoiceIdConfig::default().accept);
    assert_eq!(cfg.reject, VoiceIdConfig::default().reject);
}

#[test]
fn the_report_waits_until_the_middle_means_something() {
    let cfg = on();
    let mut id = VoiceId::default();
    for _ in 0..9 {
        id.note_accepted(0.8);
    }
    assert!(
        id.thresholds_report(&cfg).is_none(),
        "nine scores are not a distribution to measure a threshold against"
    );
}

#[test]
fn the_report_says_where_the_accept_line_sits_in_your_own_variations() {
    let cfg = on();
    let mut id = VoiceId::default();
    // A voice that usually scores ~0.80, wobbling a little.
    for i in 0..30 {
        id.note_accepted(0.78 + (i % 5) as f32 * 0.01);
    }
    let report = id.thresholds_report(&cfg).expect("thirty scores is a distribution");
    assert!(report.contains("0.80"), "the middle of the scores is named: {report}");
    assert!(report.contains("0.72"), "the accept line is named: {report}");
}

#[test]
fn forgetting_a_voice_forgets_its_score_history_too() {
    // The distribution IS a sketch of the voice. A revoke that kept it would
    // be a revoke in name.
    let mut id = VoiceId::default();
    id.enroll(&[0.1, 0.2, 0.3]).unwrap();
    id.note_accepted(0.8);
    id.forget();
    assert_eq!(id.enrolled(), 0);
    assert!(id.accepted.is_empty(), "forget left the score history behind");
}
