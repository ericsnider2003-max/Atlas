use atlas::presence::{
    interpret, may_answer, Change, Gesture, Look, Presence, PresenceConfig, Sensor, Signal,
};
use atlas::voiceid::{cosine, enrollment_prompt, handle, Handling, Verdict, VoiceId, VoiceIdConfig};

fn cfg() -> VoiceIdConfig {
    VoiceIdConfig { enabled: true, ..Default::default() }
}
fn emb(seed: f32) -> Vec<f32> {
    (0..64).map(|i| ((i as f32 * 0.37) + seed).sin()).collect()
}
fn near(v: &[f32], jitter: f32) -> Vec<f32> {
    v.iter().enumerate().map(|(i, x)| x + jitter * ((i % 7) as f32 - 3.0) * 0.01).collect()
}

// ================= knowing your voice =================

#[test]
fn nothing_is_gated_before_enrollment() {
    // A half-taught voiceprint must never start ignoring you.
    let v = VoiceId::default();
    assert_eq!(v.check(&emb(0.0), &cfg()), Verdict::NotEnrolled);
    assert_eq!(handle(Verdict::NotEnrolled, true, &cfg()), Handling::Proceed);
}

#[test]
fn one_or_two_samples_is_not_enough_to_start_judging() {
    let mut v = VoiceId::default();
    v.enroll(&emb(0.0)).unwrap();
    v.enroll(&near(&emb(0.0), 1.0)).unwrap();
    assert_eq!(v.check(&emb(0.0), &cfg()), Verdict::NotEnrolled, "needs 3");
}

#[test]
fn your_own_voice_is_recognised_after_enrollment() {
    let mut v = VoiceId::default();
    let me = emb(0.0);
    for j in 0..4 {
        v.enroll(&near(&me, j as f32)).unwrap();
    }
    assert!(matches!(v.check(&near(&me, 1.5), &cfg()), Verdict::You(_)));
}

#[test]
fn a_clearly_different_voice_is_not_you() {
    let mut v = VoiceId::default();
    for j in 0..4 {
        v.enroll(&near(&emb(0.0), j as f32)).unwrap();
    }
    let someone_else: Vec<f32> = emb(0.0).iter().map(|x| -x).collect();
    assert!(matches!(v.check(&someone_else, &cfg()), Verdict::NotYou(_)));
}

#[test]
fn an_unfamiliar_voice_asks_rather_than_going_silent() {
    // The podcast case is real, but silence was the wrong answer to it: the
    // same reading is produced by the owner with a cold. What actually tells
    // a podcast from a person is the question -- a consequential action needs
    // a spoken yes in the moment, and the podcast is not listening for it.
    assert_eq!(handle(Verdict::NotYou(0.1), false, &cfg()), Handling::Proceed);
    assert_eq!(handle(Verdict::NotYou(0.1), true, &cfg()), Handling::Confirm);
}

#[test]
fn sounding_like_you_is_never_treated_as_permission() {
    // A recording of you sounds exactly like you. Voice decides whether Atlas
    // listens, never whether it is allowed.
    assert_eq!(handle(Verdict::You(0.99), true, &cfg()), Handling::Confirm);
    assert_eq!(handle(Verdict::You(0.99), false, &cfg()), Handling::Proceed);
}

#[test]
fn a_borderline_match_never_blocks_you_out_of_ordinary_work() {
    // A cold, a bad mic, sitting further away — none of that should stop you
    // opening Chrome.
    assert_eq!(handle(Verdict::Unsure(0.6), false, &cfg()), Handling::Proceed);
    assert_eq!(handle(Verdict::Unsure(0.6), true, &cfg()), Handling::Confirm);
}

#[test]
fn turning_the_feature_off_disables_all_of_it() {
    let off = VoiceIdConfig { enabled: false, ..Default::default() };
    assert_eq!(handle(Verdict::NotYou(0.0), true, &off), Handling::Proceed);
}

#[test]
fn the_voiceprint_adapts_so_a_cold_does_not_lock_you_out() {
    let mut v = VoiceId::default();
    let me = emb(0.0);
    for j in 0..4 {
        v.enroll(&near(&me, j as f32)).unwrap();
    }
    for _ in 0..40 {
        v.adapt(&near(&me, 2.0), &cfg());
    }
    assert!(v.enrolled() <= 20, "but the history stays bounded");
}

#[test]
fn changing_the_encoder_is_caught_rather_than_producing_nonsense() {
    let mut v = VoiceId::default();
    v.enroll(&emb(0.0)).unwrap();
    // Never mixed: a new encoder starts the print again (30 Sep 2026: the
    // trained model arriving had to be enrollable, not refused forever).
    assert_eq!(v.enroll(&vec![0.1; 32]).unwrap(), 1);
    assert_eq!(v.print.as_ref().unwrap().centroid.len(), 32);
}

#[test]
fn you_can_be_forgotten() {
    let mut v = VoiceId::default();
    v.enroll(&emb(0.0)).unwrap();
    v.forget();
    assert_eq!(v.enrolled(), 0);
}

#[test]
fn similarity_ignores_loudness() {
    let a = emb(0.0);
    let loud: Vec<f32> = a.iter().map(|x| x * 8.0).collect();
    assert!(cosine(&a, &loud) > 0.999, "distance from the mic must not matter");
    assert_eq!(cosine(&a, &[]), 0.0);
}

#[test]
fn enrollment_talks_you_through_it() {
    assert!(enrollment_prompt(0, 3).contains("normal voice"));
    assert!(enrollment_prompt(1, 3).contains("1 of 3"));
    assert!(enrollment_prompt(3, 3).contains("know your voice"));
}

// ================= knowing you're there =================

fn pcfg() -> PresenceConfig {
    PresenceConfig { enabled: true, ..Default::default() }
}
fn you() -> Look {
    Look { faces: 1, you: true }
}
fn empty() -> Look {
    Look { faces: 0, you: false }
}

#[test]
fn one_missed_frame_does_not_mean_you_left() {
    // You reached for a coffee. Flipping to unattended mode here would be
    // exactly the wrong call.
    let mut s = Sensor::new(pcfg());
    s.observe(you(), 0);
    assert_eq!(s.observe(empty(), 20), Change::None);
    assert_eq!(s.state, Presence::AtDesk);
}

#[test]
fn three_empty_looks_means_you_actually_left() {
    let mut s = Sensor::new(pcfg());
    s.observe(you(), 0);
    s.observe(empty(), 20);
    s.observe(empty(), 40);
    assert_eq!(s.observe(empty(), 60), Change::Left);
    assert_eq!(s.state, Presence::Away);
}

#[test]
fn coming_back_is_a_single_clear_event() {
    let mut s = Sensor::new(pcfg());
    for t in 0..4 {
        s.observe(empty(), t * 20);
    }
    assert_eq!(s.observe(you(), 100), Change::Returned);
    assert_eq!(s.state, Presence::AtDesk);
    assert_eq!(s.observe(you(), 120), Change::None, "reported once, not every sample");
}

#[test]
fn someone_else_at_your_desk_is_noticed() {
    let mut s = Sensor::new(pcfg());
    s.observe(you(), 0);
    let c = s.observe(Look { faces: 2, you: true }, 20);
    assert_eq!(c, Change::StrangerArrived);
    assert_eq!(s.state, Presence::NotAlone);
    assert!(s.state.should_be_discreet(), "private things stay quiet");
    assert!(s.state.here(), "you are still there though");
}

#[test]
fn a_face_that_is_not_yours_is_not_you_being_present() {
    let mut s = Sensor::new(pcfg());
    s.observe(Look { faces: 1, you: false }, 0);
    assert_eq!(s.state, Presence::Stranger);
    assert!(!s.state.here());
    assert!(s.state.should_be_discreet());
}

#[test]
fn a_blind_camera_means_carry_on_as_normal_not_nobody_is_there() {
    // Treating "no camera" as "empty room" would silence Atlas whenever the
    // webcam is covered.
    let mut s = Sensor::new(pcfg());
    s.observe(you(), 0);
    s.blind();
    assert_eq!(s.state, Presence::Unknown);
    assert!(s.state.worth_speaking());
    assert!(!s.state.should_be_discreet());
}

#[test]
fn the_camera_is_off_by_default_and_sampled_slowly_when_on() {
    let off = Sensor::default();
    assert!(!off.due(999_999), "must not sample unless enabled");
    let on = Sensor::new(pcfg());
    assert!(!on.due(5), "not a video feed");
    assert!(on.due(60));
}

#[test]
fn atlas_does_not_talk_to_an_empty_room() {
    let mut s = Sensor::new(pcfg());
    for t in 0..4 {
        s.observe(empty(), t * 20);
    }
    assert!(!s.state.worth_speaking());
}

// ================= silent answers =================

#[test]
fn a_thumb_answers_a_question_atlas_already_asked() {
    assert_eq!(interpret(Gesture::ThumbUp, true, false), Signal::Yes);
    assert_eq!(interpret(Gesture::ThumbDown, true, false), Signal::No);
}

#[test]
fn a_gesture_is_never_a_command_on_its_own() {
    // A misread gesture that opens an app is confusing. One that approves a
    // post is unacceptable.
    assert_eq!(interpret(Gesture::ThumbUp, false, false), Signal::Ignored);
    assert_eq!(interpret(Gesture::ThumbDown, false, false), Signal::Ignored);
}

#[test]
fn a_raised_palm_stops_atlas_talking_without_saying_a_word() {
    // The genuinely useful case: you are on a call and Atlas starts speaking.
    assert_eq!(interpret(Gesture::OpenPalm, false, true), Signal::Stop);
    assert_eq!(interpret(Gesture::OpenPalm, false, false), Signal::Ignored);
}

#[test]
fn a_thumbs_up_cannot_approve_something_consequential() {
    // Two fingers of confidence from a webcam is not consent to post.
    assert!(!may_answer(Signal::Yes, true));
    assert!(may_answer(Signal::Yes, false));
}

#[test]
fn declining_by_gesture_is_always_allowed() {
    assert!(may_answer(Signal::No, true), "saying no is always safe");
    assert!(may_answer(Signal::Stop, true));
}

#[test]
fn the_shipped_config_keeps_both_sensors_off_until_you_ask() {
    let y = std::fs::read_to_string("config/tools.yaml").unwrap();
    let t: atlas::voice::ToolsConfig = serde_yaml::from_str(&y).unwrap();
    assert!(!t.voice_id.enabled, "a microphone filter must be opt-in");
    assert!(!t.presence.enabled, "a camera must be opt-in");
    assert!(t.voice_id.accept > t.voice_id.reject, "there must be a grey band");
    assert!(t.presence.leave_after > 1, "one missed frame must not mean you left");
}
