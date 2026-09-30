//! Eric, 29 Sep 2026: "I feel like I have to yell to get Atlas to hear me."
//!
//! **The evidence.** His laptop's `hearing` record: the microphone in use,
//! "Microphone (HD Pro Webcam C920)", measured -90.3 dB in a quiet room; the
//! laptop's Intel array -61.6. And the check every recording passes before
//! the speech engine (`audio::check_speech`) called nothing below -62 dBFS
//! speech, "however quiet the room" -- so a normal voice on a microphone set
//! low (-60 to -65 dB) was silence, and the same words shouted (-45) were
//! heard. The microphone choice ranked by a second of an empty room, so the
//! webcam mic, gating to near-silence between words, was "dead" and the
//! noisier laptop mic "better".
//!
//! **Now** speech is judged by how far it stands above the room, the clip is
//! levelled to where whisper hears it best by your usual voice on that
//! microphone (`leveller`), microphones are ranked by your voice over their
//! room, and a microphone set too low is said plainly (and on Windows raised
//! once, `miclevel`).
//!
//! The voices are real synthesized speech (espeak-ng, the fixtures under
//! tests/fixtures/speech), scaled to the level a quiet microphone gives and
//! laid over a room at the level that microphone's room reads. The whole path
//! is run as the running Atlas runs it: the stream cut where you pause
//! (`utterance::Segmenter`), then what the speech engine would be handed
//! (`leveller::for_speech_to_text`). Whisper itself isn't on this machine;
//! `the_name_and_the_request_in_one_breath` runs it where it is.

use atlas::audio::{level_db, Device, Kind};
use atlas::endpoint::EndpointConfig;
use atlas::hearing::{Ear, Hearing, HearingConfig, Where};
use atlas::leveller::{self, Leveller, Measured};
use atlas::utterance::{Seg, Segmenter, WINDOW};

const RATE: u32 = 16_000;
const C920: &str = "Microphone (HD Pro Webcam C920)";
const INTEL: &str = "Microphone Array (Intel® Smart Sound Technology for Digital Microphones)";

fn corpus(name: &str) -> Vec<i16> {
    let (s, r) = atlas::diarize::read_wav(&std::fs::read(format!("tests/fixtures/speech/corpus/{name}.wav")).unwrap()).unwrap();
    assert_eq!(r, RATE);
    s
}

/// Coloured noise at about `db` dBFS RMS.
fn room(ms: u32, db: f32, seed: u64) -> Vec<f32> {
    room_n((ms * RATE / 1000) as usize, db, seed)
}

fn room_n(n: usize, db: f32, seed: u64) -> Vec<f32> {
    let mut x = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    let mut lp = 0f32;
    let raw: Vec<f32> = (0..n)
        .map(|_| {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let w = ((x >> 33) as f32 / (1u64 << 31) as f32) - 1.0;
            lp = 0.8 * lp + 0.2 * w;
            lp
        })
        .collect();
    let rms = (raw.iter().map(|v| v * v).sum::<f32>() / raw.len() as f32).sqrt().max(1e-9);
    let want = 10f32.powf(db / 20.0) * 32767.0;
    raw.iter().map(|v| v / rms * want).collect()
}

/// The voice's own level: energy mean of the frames within 30 dB of its
/// loudest (the words, not the gaps between them).
fn voice_db(s: &[i16]) -> f32 {
    let levels: Vec<f32> = s.chunks(480).filter(|c| c.len() == 480).map(level_db).collect();
    let top = levels.iter().cloned().fold(f32::MIN, f32::max);
    let voiced: Vec<f32> = levels.into_iter().filter(|l| *l > top - 30.0).collect();
    let e = voiced.iter().map(|l| 10f64.powf(*l as f64 / 10.0)).sum::<f64>() / voiced.len() as f64;
    (10.0 * e.log10()) as f32
}

/// Real speech at `speech_db`, with a second of the room either side, over a
/// room at `room_db` throughout.
fn said_at(clip: &str, speech_db: f32, room_db: f32, seed: u64) -> Vec<i16> {
    let voice = corpus(clip);
    let g = 10f32.powf((speech_db - voice_db(&voice)) / 20.0);
    let lead = 1_000 * RATE as usize / 1000;
    let total = lead * 2 + voice.len();
    let bed = room_n(total, room_db, seed);
    (0..total)
        .map(|i| {
            let v = if i >= lead && i < lead + voice.len() { voice[i - lead] as f32 * g } else { 0.0 };
            (v + bed[i]).clamp(-32768.0, 32767.0) as i16
        })
        .collect()
}

/// What the stream cutter hands on, as the wake word's stream runs it.
fn cut(stream: &[i16]) -> Vec<Vec<i16>> {
    let mut seg = Segmenter::new(&EndpointConfig::default());
    let mut out = Vec::new();
    for w in stream.chunks_exact(WINDOW) {
        if let Seg::Done(u) = seg.feed(w) {
            out.push(u);
        }
    }
    if let Some(u) = seg.finish() {
        out.push(u);
    }
    out
}

/// The whole way from microphone to what whisper would be handed.
fn heard(stream: &[i16], lev: &mut Leveller, mic: &str) -> Vec<Vec<i16>> {
    cut(stream).into_iter().filter_map(|u| leveller::for_speech_to_text(&u, RATE, mic, lev)).collect()
}

#[test]
fn a_normal_voice_on_a_microphone_set_low_is_heard_without_shouting() {
    // (speech level, the room under it): a microphone set low turns the
    // room down with the voice. The last two are Eric's webcam mic, whose
    // room reads -90.
    for (speech, room_db) in [(-40.0, -68.0), (-50.0, -76.0), (-60.0, -85.0), (-68.0, -90.0)] {
        for clip in ["A_s0", "B_s3"] {
            let mut lev = Leveller::default();
            let stream = said_at(clip, speech, room_db, 7);
            let got = heard(&stream, &mut lev, C920);
            assert_eq!(got.len(), 1, "{clip} at {speech} dB over {room_db}: {} utterances reached the speech engine", got.len());
            let level = voice_db(&got[0]);
            // Levelled to where whisper hears it, within the room's ceiling.
            let expect = (leveller::TARGET_SPEECH_DB).min(speech + (leveller::NOISE_CEILING_DB - room_db)).min(speech + leveller::MAX_GAIN_DB);
            assert!(
                (level - expect).abs() < 4.0 && level > -35.0,
                "{clip} at {speech} dB reached whisper at {level:.1} dB; expected about {expect:.1}"
            );
            // And the room under it is never lifted past the ceiling.
            let room_after = level_db(&got[0][..1600]);
            assert!(room_after < leveller::NOISE_CEILING_DB + 6.0, "the room came up to {room_after:.1} dB");
        }
    }
}

#[test]
fn the_old_fixed_line_is_what_made_you_shout() {
    // A voice at -68 dB over a -90 dB room, against the -62 line the check
    // used until today: only its loudest syllables reach that line, for
    // less than the quarter second of speech it asks for, so it was silence
    // -- and 20 dB louder, the same words got through.
    let quiet = said_at("A_s1", -68.0, -90.0, 3);
    let loud = said_at("A_s1", -48.0, -70.0, 3);
    let frames_over = |s: &[i16], line: f32| s.chunks(480).filter(|c| c.len() == 480).filter(|c| level_db(c) >= line).count();
    assert!(frames_over(&quiet, -62.0) * 30 < atlas::audio::MIN_SPEECH_MS as usize, "the fixture isn't quiet enough to show it");
    assert!(frames_over(&loud, -62.0) * 30 >= atlas::audio::MIN_SPEECH_MS as usize);
    // Now: heard.
    assert_ne!(atlas::audio::check_speech(&quiet, RATE), atlas::audio::SpeechCheck::Silence);
}

#[test]
fn noise_is_not_speech_at_any_level() {
    let mut lev = Leveller::default();
    // A steady room, loud and quiet; a hum; a room with a door knock.
    let mut streams: Vec<(String, Vec<i16>)> = Vec::new();
    for db in [-40.0, -55.0, -70.0, -88.0] {
        streams.push((format!("room at {db}"), room(6_000, db, 11).into_iter().map(|v| v as i16).collect()));
    }
    let hum: Vec<i16> = (0..RATE as usize * 6).map(|i| ((i as f32 * 2.0 * std::f32::consts::PI * 60.0 / RATE as f32).sin() * 300.0) as i16).collect();
    streams.push(("mains hum".into(), hum));
    let mut knock: Vec<i16> = room(3_000, -70.0, 5).into_iter().map(|v| v as i16).collect();
    knock.extend((0..480).map(|i| (((i * 7919) % 2000) as i32 - 1000).clamp(-32768, 32767) as i16 * 8));
    knock.extend(room(3_000, -70.0, 6).into_iter().map(|v| v as i16));
    streams.push(("a knock".into(), knock));
    // A dead or muted microphone: zeros with a least-significant-bit of dither.
    let dead: Vec<i16> = (0..RATE as usize * 6).map(|i| ((i * 7919) % 3) as i16 - 1).collect();
    streams.push(("a dead microphone".into(), dead));
    for (what, s) in streams {
        let got = heard(&s, &mut lev, C920);
        assert!(got.is_empty(), "{what}: {} clip(s) would have gone to the speech engine", got.len());
    }
    assert_eq!(lev.heard_count(C920), 0, "noise must not teach the leveller a voice level");
}

#[test]
fn the_gain_follows_your_usual_voice_not_one_loud_syllable() {
    let mut lev = Leveller::default();
    for (i, clip) in ["A_s0", "A_s1", "A_s2", "B_s0"].iter().enumerate() {
        let s = said_at(clip, -58.0, -85.0, i as u64);
        assert_eq!(heard(&s, &mut lev, C920).len(), 1);
    }
    let usual = lev.speech_db(C920).unwrap();
    assert!((usual - -58.0).abs() < 4.0, "{usual}");
    // A cough-loud clip is levelled by the usual voice, and its peaks are
    // held under full scale rather than wrapped.
    let loud = said_at("B_s1", -30.0, -85.0, 9);
    let m = leveller::measure_voice(&loud, RATE).unwrap();
    let g = lev.gain_for(C920, m);
    let out = leveller::levelled(&loud, g);
    assert!(out.iter().all(|s| s.unsigned_abs() < 32767), "clipped");
    // Quiet input is noticed after a few turns, and said plainly.
    let typical = lev.quiet_input(C920).expect("-58 dB is a microphone set too low");
    let line = leveller::quiet_input_line("the webcam", typical, Some(atlas::miclevel::InputLevel { scalar: 0.06, muted: false }), Some(0.8));
    assert!(line.contains("6%") && line.contains("80%") && line.contains("Sound settings"), "{line}");
    assert!(!line.contains("atlas "), "no commands in what's said: {line}");
    // A normal level isn't nagged about.
    let mut fine = Leveller::default();
    for i in 0..4 {
        fine.heard(INTEL, Measured { speech_db: -30.0, noise_db: -60.0 - i as f32 });
    }
    assert_eq!(fine.quiet_input(INTEL), None);
}

#[test]
fn the_microphone_that_hears_your_voice_best_is_chosen_not_the_loudest_room() {
    // Eric's two microphones as his laptop measured them.
    let devices = vec![Device::new(C920, Kind::Input), Device::new(INTEL, Kind::Input)];
    let cfg = HearingConfig::default();
    let w = Where { at_desk: true, presence_unknown: false, headset_connected: false, phone_active: false, audio_playing: false };
    let mut h = Hearing::default();
    h.observe_devices(&devices);
    h.record_level(C920, -90.3, 100);
    h.record_level(INTEL, -61.6, 100);
    // Before any voice is heard: a second of room is all there is, and the
    // webcam mic reads as dead.
    assert_eq!(h.clone().decide(&w, &cfg, 200).ear, Ear::Desk(INTEL.into()));
    // What his voice measured on each: 26 dB over the room on the webcam,
    // 9 dB on the laptop's array.
    let mut lev = Leveller::default();
    for _ in 0..3 {
        lev.heard(C920, Measured { speech_db: -62.0, noise_db: -88.0 });
        lev.heard(INTEL, Measured { speech_db: -51.0, noise_db: -60.0 });
    }
    let mut h2 = h.clone();
    h2.learn_levels(&lev);
    assert_eq!(h2.snr_of(C920), Some(26.0));
    let c = h2.decide(&w, &cfg, 300);
    assert_eq!(c.ear, Ear::Desk(C920.into()), "{}", c.why);
    // And it is not listed as a microphone that can't hear you.
    assert!(h2.deaf_devices(&cfg).iter().all(|d| d.name != C920));
}
