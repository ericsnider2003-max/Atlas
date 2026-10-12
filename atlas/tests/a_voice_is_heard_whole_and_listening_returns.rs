//! Eric, 1 Oct 2026: Atlas is slow, doesn't hear him (up to five tries), hears
//! half a sentence, and sometimes stops listening. This replays synthetic
//! speech over synthetic rooms through the real path (`Segmenter`,
//! `Endpointer`, `check_speech`) and holds: the whole utterance is captured,
//! a mid-sentence pause doesn't cut it, the wait after it is bounded, a room
//! alone is never an utterance, and every ending returns to listening.

use atlas::audio::{check_speech, level_db, SpeechCheck};
use atlas::endpoint::{shape_of, EndpointConfig, Endpointer, Listening, Shape, Why};
use atlas::utterance::{Seg, Segmenter, WINDOW};

const RATE: usize = 16_000;
const WIN_MS: usize = 32;

fn ms(n: usize) -> usize {
    n * RATE / 1000
}

fn scaled(raw: Vec<f32>, target_db: f32) -> Vec<i16> {
    let rms = (raw.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / raw.len().max(1) as f64).sqrt();
    let want = 32768.0 * 10f64.powf(target_db as f64 / 20.0);
    raw.iter().map(|x| (*x as f64 / rms * want).round().clamp(-32768.0, 32767.0) as i16).collect()
}

fn noise(n_ms: usize, db: f32, seed: &mut u64) -> Vec<i16> {
    let raw = (0..ms(n_ms))
        .map(|_| {
            *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((*seed >> 33) as f32 / (1u64 << 31) as f32) - 0.5
        })
        .collect();
    scaled(raw, db)
}

/// A voiced burst: harmonics of a moving pitch, with a syllable rhythm.
fn voice(n_ms: usize, db: f32) -> Vec<i16> {
    let mut phase = 0.0f32;
    let raw = (0..ms(n_ms))
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let f0 = 120.0 + 25.0 * (t * 3.0).sin();
            phase += 2.0 * std::f32::consts::PI * f0 / RATE as f32;
            let s: f32 = (1..=12).map(|h| (phase * h as f32).sin() / h as f32).sum();
            s * (0.6 + 0.4 * (t * 2.0 * std::f32::consts::PI * 4.0).sin().abs())
        })
        .collect();
    scaled(raw, db)
}

/// Speech `bursts` (ms, dB relative) separated by `pauses` (ms) over a room,
/// with a leading and trailing stretch of room. Returns (stream, speech span ms).
fn utterance(speech_db: f32, room_db: f32, bursts: &[(usize, f32)], pauses: &[usize], trail_ms: usize) -> (Vec<i16>, usize) {
    let mut seed = 7u64;
    let mut s = noise(700, room_db, &mut seed);
    let mut span = 0;
    for (i, (len, rel)) in bursts.iter().enumerate() {
        let v = voice(*len, speech_db + rel);
        let n = noise(*len, room_db, &mut seed);
        s.extend(v.iter().zip(&n).map(|(a, b)| a.saturating_add(*b)));
        span += len;
        if let Some(p) = pauses.get(i) {
            s.extend(noise(*p, room_db, &mut seed));
            span += p;
        }
    }
    s.extend(noise(trail_ms, room_db, &mut seed));
    (s, span)
}

fn run(stream: &[i16]) -> Vec<Vec<i16>> {
    let mut seg = Segmenter::new(&EndpointConfig::default());
    let mut done = Vec::new();
    for w in stream.chunks_exact(WINDOW) {
        if let Seg::Done(u) = seg.feed(w) {
            done.push(u);
        }
    }
    done
}

#[test]
fn a_whole_utterance_with_pauses_and_a_soft_ending_is_one_utterance() {
    let mut failures = Vec::new();
    for speech_db in [-20.0f32, -30.0, -40.0, -50.0] {
        for room_db in [-60.0f32, -75.0, -90.0] {
            let snr = speech_db - room_db;
            if snr < 25.0 {
                continue; // too close to the room for a soft ending to be fair
            }
            let bursts = [(900usize, 0.0f32), (500, 0.0), (450, -6.0)];
            let pauses = [400usize, 600];
            let (s, span) = utterance(speech_db, room_db, &bursts, &pauses, 2000);
            let done = run(&s);
            if done.len() != 1 {
                failures.push(format!("speech {speech_db} room {room_db}: {} utterances", done.len()));
                continue;
            }
            let got = done[0].len() * 1000 / RATE;
            if got + 40 < span {
                failures.push(format!("speech {speech_db} room {room_db}: captured {got} ms of {span} ms"));
            }
            if got > span + 480 + 96 + 300 + 100 {
                failures.push(format!("speech {speech_db} room {room_db}: {got} ms for {span} ms of speech"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn the_wait_after_you_stop_is_bounded() {
    let cfg = EndpointConfig::default();
    for (speech_db, room_db) in [(-30.0f32, -75.0f32), (-40.0, -90.0), (-20.0, -60.0)] {
        let (s, _) = utterance(speech_db, room_db, &[(1200, 0.0)], &[], 3000);
        let speech_end = (700 + 1200) / WIN_MS;
        let mut seg = Segmenter::new(&cfg);
        let mut at = None;
        for (i, w) in s.chunks_exact(WINDOW).enumerate() {
            if matches!(seg.feed(w), Seg::Done(_)) {
                at = Some(i);
                break;
            }
        }
        let at = at.unwrap_or_else(|| panic!("never finished at speech {speech_db} room {room_db}"));
        let waited = (at.saturating_sub(speech_end) * WIN_MS) as u64;
        assert!(waited <= cfg.sentence_ms + 160, "waited {waited} ms at speech {speech_db} room {room_db}");
    }
}

#[test]
fn a_room_alone_is_never_an_utterance() {
    let mut seed = 99u64;
    for room_db in [-60.0f32, -75.0, -90.0] {
        let s = noise(8000, room_db, &mut seed);
        assert!(run(&s).is_empty(), "room {room_db} read as speech");
        assert_eq!(check_speech(&s, RATE as u32), SpeechCheck::Silence, "room {room_db} passed check_speech");
    }
}

#[test]
fn a_soft_flicker_still_starts_an_utterance() {
    // Speech that dips under the detector now and then (every third window).
    let mut seed = 5u64;
    let mut s = noise(700, -75.0, &mut seed);
    let v = voice(1500, -35.0);
    for (i, w) in v.chunks_exact(WINDOW).enumerate() {
        if i % 3 == 2 {
            s.extend(noise(WIN_MS, -75.0, &mut seed));
        } else {
            s.extend_from_slice(w);
        }
    }
    s.extend(noise(2000, -75.0, &mut seed));
    assert_eq!(run(&s).len(), 1);
}

#[test]
fn a_clip_must_stand_well_clear_of_its_room() {
    let mut seed = 3u64;
    // 8 dB over the room: a fan swelling, not a person.
    let mut weak = noise(600, -70.0, &mut seed);
    weak.extend(noise(600, -62.0, &mut seed));
    weak.extend(noise(600, -70.0, &mut seed));
    assert_eq!(check_speech(&weak, RATE as u32), SpeechCheck::Silence);
    // A normal voice on a quiet mic: 22 dB over, still heard.
    let (s, _) = utterance(-68.0, -90.0, &[(800, 0.0)], &[], 600);
    assert_ne!(check_speech(&s, RATE as u32), SpeechCheck::Silence);
    // A short word survives.
    let (s, _) = utterance(-40.0, -75.0, &[(220, 0.0)], &[], 600);
    assert_ne!(check_speech(&s, RATE as u32), SpeechCheck::Silence);
}

#[test]
fn the_gain_is_capped_unless_the_signal_is_clean() {
    let mut seed = 11u64;
    // 17 dB over the room, very quiet: gain may not exceed 25 dB.
    let mut s = noise(500, -90.0, &mut seed);
    let v = voice(800, -73.0);
    let n = noise(800, -90.0, &mut seed);
    s.extend(v.iter().zip(&n).map(|(a, b)| a.saturating_add(*b)));
    s.extend(noise(500, -90.0, &mut seed));
    match check_speech(&s, RATE as u32) {
        SpeechCheck::Quiet(g) => assert!(20.0 * g.log10() <= 25.01, "gain {} dB", 20.0 * g.log10()),
        other => panic!("{other:?}"),
    }
}

#[test]
fn what_time_is_it_does_not_wait_for_more() {
    let cfg = EndpointConfig::default();
    assert_eq!(shape_of("what time is it"), Shape::Sentence);
    assert_eq!(shape_of("what time is it?"), Shape::Complete);
    assert_eq!(shape_of("Set a timer for ten minutes."), Shape::Complete);
    assert_eq!(shape_of("remind me to call Maya,"), Shape::MidPhrase);
    assert_eq!(shape_of("remind me to"), Shape::MidPhrase);
    assert!(shape_of("what time is it?").silence_needed(&cfg) <= 500);
    // Empty text, short speech: the short wait.
    let mut ep = Endpointer::start(0);
    let mut t = 0u64;
    for _ in 0..20 {
        t += 32;
        ep.feed(-20.0, "", t, &cfg);
    }
    let mut waited = 0;
    while !ep.finished() && waited < 2000 {
        t += 32;
        ep.feed(-80.0, "", t, &cfg);
        waited += 32;
    }
    assert!(waited <= 700, "waited {waited}");
}

#[test]
fn it_always_returns_to_listening() {
    let cfg = EndpointConfig::default();
    let drive = |ep: &mut Endpointer, t: &mut u64, db: f32, for_ms: u64| {
        let end = *t + for_ms;
        while *t < end && !ep.finished() {
            *t += 32;
            ep.feed(db, "", *t, &cfg);
        }
    };
    let mut t = 0u64;
    let mut ep = Endpointer::start(t);
    drive(&mut ep, &mut t, -80.0, 10_000);
    assert_eq!(ep.state, Listening::Finished(Why::Nothing));

    let mut ep = Endpointer::start(t);
    drive(&mut ep, &mut t, -20.0, 30_000);
    assert_eq!(ep.state, Listening::Finished(Why::RanTooLong));

    let mut ep = Endpointer::start(t);
    drive(&mut ep, &mut t, -20.0, 800);
    drive(&mut ep, &mut t, -80.0, 3000);
    assert_eq!(ep.state, Listening::Finished(Why::YouFinished));

    // After each, a fresh one works end to end.
    let mut ep = Endpointer::start(t);
    assert_eq!(ep.state, Listening::Waiting);
    drive(&mut ep, &mut t, -20.0, 700);
    assert_eq!(ep.state, Listening::Speaking);
    drive(&mut ep, &mut t, -80.0, 3000);
    assert_eq!(ep.state, Listening::Finished(Why::YouFinished));
}

#[test]
fn the_segmenter_hears_the_next_utterance_after_a_stuck_one() {
    let mut seed = 1u64;
    let mut s = noise(700, -75.0, &mut seed);
    // 22 s of continuous sound: the hard stop ends it.
    s.extend(voice(22_000, -30.0));
    s.extend(noise(1500, -75.0, &mut seed));
    let (next, _) = utterance(-30.0, -75.0, &[(900, 0.0)], &[], 1500);
    s.extend(next);
    let done = run(&s);
    assert!(done.len() >= 2, "{} utterances", done.len());
    let last = done.last().unwrap();
    assert!(level_db(last) > -60.0);
}
