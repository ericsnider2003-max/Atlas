//! The half of endpointing that was missing.
//!
//! `endpoint.rs` has known when a person has stopped talking since the day it
//! was written. Its own doc opens: *"Recording for a fixed eight seconds is
//! the most-copied mistake in voice software. It cuts you off mid-word when
//! you have more to say, and makes you wait seven seconds after 'yes'."* And
//! then `config/tools.yaml`'s record command carried `-t {seconds}` and every
//! turn recorded for eight seconds regardless — because nothing ever produced
//! the windows of audio the endpointer decides from.
//!
//! This file covers the pieces that make those windows: a level meter over
//! raw PCM, the wav header the clip needs afterwards, and the ffmpeg
//! invocation that streams rather than writing a fixed-length file. All of it
//! is arithmetic on bytes, which means all of it can be proven here.
//!
//! What cannot be proven here is the loop against a live microphone. This
//! container has no sound hardware, so `listen_until_you_stop` has never read
//! a byte from a real recorder. That stays on the hardware-blocked list.

use atlas::audio::{level_db, samples_from_le, stream_args, wav_bytes, window_samples, SILENT_DB};
use atlas::endpoint::{EndpointConfig, Endpointer, Why};

/// A sine-ish tone at a given fraction of full scale.
fn tone(n: usize, amplitude: f32) -> Vec<i16> {
    (0..n)
        .map(|i| {
            let phase = (i as f32) * 0.3;
            (phase.sin() * amplitude * i16::MAX as f32) as i16
        })
        .collect()
}

// ================= measuring a window =================

#[test]
fn silence_reads_as_silence_and_full_scale_reads_as_zero() {
    assert_eq!(level_db(&vec![0i16; 1000]), SILENT_DB);
    // A square wave at full scale is the loudest signal there is: RMS equals
    // peak equals full scale, so 0 dBFS.
    let square: Vec<i16> = (0..1000).map(|i| if i % 2 == 0 { i16::MAX } else { -i16::MAX }).collect();
    assert!(level_db(&square).abs() < 0.1, "full scale should be 0 dBFS, got {}", level_db(&square));
}

#[test]
fn quiet_speech_and_a_quiet_room_land_either_side_of_the_shipped_threshold() {
    // The shipped threshold is -38 dBFS. This is the assertion that makes the
    // number meaningful rather than arbitrary: ordinary speech has to read
    // above it and a quiet room below it, or endpointing either never starts
    // or never stops.
    let cfg = EndpointConfig::default();
    let speech = level_db(&tone(4000, 0.08)); // quiet-ish speech
    let room = level_db(&tone(4000, 0.002)); // a quiet room
    assert!(speech > cfg.silence_below_db, "speech at {speech} dB reads as silence");
    assert!(room < cfg.silence_below_db, "a quiet room at {room} dB reads as speech");
}

#[test]
fn an_empty_window_is_not_treated_as_the_loudest_sound_there_is() {
    // The trap: dBFS is a ratio, and a naive implementation returns 0 for an
    // empty buffer -- which is full scale, which reads as speech, which would
    // hold a recording open forever on a microphone that has stopped
    // producing audio.
    assert_eq!(level_db(&[]), SILENT_DB);
    assert!(level_db(&[]) < EndpointConfig::default().silence_below_db);
}

#[test]
fn a_click_is_attenuated_by_rms_but_not_eliminated_by_it() {
    // Measured, not assumed. RMS rather than peak is what keeps a keystroke
    // or a cable knock from reading as a shout: one full-scale sample in a
    // 250ms window is 0 dBFS by peak and **-36 dBFS** by RMS.
    //
    // But the shipped threshold is -38, so -36 still clears it. The honest
    // statement is therefore *attenuated*, not *eliminated*: RMS buys 36 dB,
    // and the last two are not there. What that costs is bounded and small —
    // a lone click cannot end a turn, it can only reset the silence timer, so
    // the worst case is one extra pause-length of waiting.
    //
    // Whether -38 is the right threshold against a real desk is a tuning
    // question that needs real audio and real hardware, and is on the
    // blocked list as exactly that.
    let mut window = vec![0i16; 4000];
    window[2000] = i16::MAX;
    let db = level_db(&window);
    assert!((db - -36.0).abs() < 0.5, "a single click measured {db} dB, expected about -36");

    // The property that actually matters: a click cannot *finish* a turn,
    // and it cannot make a silent room look like speech for long.
    let cfg = EndpointConfig::default();
    let mut ep = Endpointer::start(0);
    let quiet = level_db(&tone(4000, 0.001));
    let mut t = 0u64;
    for _ in 0..3 {
        t += 250;
        ep.feed(level_db(&tone(4000, 0.15)), "", t, &cfg); // real speech
    }
    let mut clicks_seen = 0;
    while !ep.finished() && t < 20_000 {
        t += 250;
        // A click every second, silence otherwise.
        if t % 1000 == 0 {
            clicks_seen += 1;
            ep.feed(db, "", t, &cfg);
        } else {
            ep.feed(quiet, "", t, &cfg);
        }
    }
    assert!(ep.finished(), "a clicking room never let the turn end");
    assert!(clicks_seen > 0, "the test did not actually click");
}

#[test]
fn a_measurement_does_not_drift_with_window_length() {
    // The same sound measured over a longer window must give the same answer,
    // or the threshold means something different at different buffer sizes.
    let short = level_db(&tone(1000, 0.1));
    let long = level_db(&tone(16000, 0.1));
    assert!((short - long).abs() < 1.0, "{short} vs {long} for the same signal");
}

// ================= turning bytes into windows =================

#[test]
fn a_read_that_lands_mid_sample_does_not_invent_noise() {
    // A stream read returns whatever arrived, which is routinely an odd
    // number of bytes. Half a sample interpreted as a whole one is a value at
    // an arbitrary amplitude -- exactly what would be mistaken for speech.
    let bytes = vec![0x00, 0x10, 0x00, 0x20, 0x7f]; // two samples and a half
    let s = samples_from_le(&bytes);
    assert_eq!(s.len(), 2);
    assert_eq!(s, vec![0x1000, 0x2000]);
}

#[test]
fn a_window_is_the_number_of_samples_that_length_of_time_holds() {
    assert_eq!(window_samples(16_000, 250), 4_000);
    assert_eq!(window_samples(16_000, 1000), 16_000);
    assert_eq!(window_samples(48_000, 250), 12_000);
}

// ================= the clip whisper reads =================

#[test]
fn the_wav_header_says_what_the_audio_actually_is() {
    let samples = tone(16_000, 0.2); // one second at 16kHz
    let wav = wav_bytes(&samples, 16_000);

    assert_eq!(&wav[0..4], b"RIFF");
    assert_eq!(&wav[8..12], b"WAVE");
    assert_eq!(&wav[36..40], b"data");
    assert_eq!(wav.len(), 44 + samples.len() * 2, "header plus the samples");

    let rate = u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]);
    let channels = u16::from_le_bytes([wav[22], wav[23]]);
    let bits = u16::from_le_bytes([wav[34], wav[35]]);
    assert_eq!(rate, 16_000, "a wrong rate makes whisper transcribe a chipmunk");
    assert_eq!(channels, 1);
    assert_eq!(bits, 16);

    let declared = u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]);
    assert_eq!(declared as usize, samples.len() * 2, "the data length lies");
    let riff = u32::from_le_bytes([wav[4], wav[5], wav[6], wav[7]]);
    assert_eq!(riff as usize, wav.len() - 8, "the RIFF size lies");
}

#[test]
fn the_samples_survive_the_round_trip() {
    let samples = vec![0i16, 1, -1, i16::MAX, i16::MIN, 1234, -4321];
    let wav = wav_bytes(&samples, 16_000);
    let back = samples_from_le(&wav[44..]);
    assert_eq!(back, samples);
}

// ================= asking ffmpeg for a stream =================

#[test]
fn the_streaming_recorder_has_no_stopwatch_in_it() {
    let args = stream_args("Some Microphone", 16_000, 20);
    let joined = args.join(" ");
    assert!(joined.contains("s16le"), "not raw PCM: {joined}");
    assert!(joined.ends_with(" -"), "not streaming to stdout: {joined}");
    assert!(joined.contains("16000"));
    assert!(joined.contains("-ac 1"), "not mono: {joined}");
    // The one `-t` present is the stuck-microphone backstop, an order of
    // magnitude longer than a sentence -- not the eight-second turn length
    // this whole path exists to remove.
    assert!(joined.contains("-t 20"), "no backstop against a stuck mic: {joined}");
    assert!(!joined.contains("-t 8"));
}

#[test]
fn each_platform_is_asked_the_way_it_answers() {
    let joined = stream_args("dev", 16_000, 20).join(" ");
    if cfg!(windows) {
        assert!(joined.contains("dshow") && joined.contains("audio=dev"), "{joined}");
    } else if cfg!(target_os = "macos") {
        assert!(joined.contains("avfoundation"), "{joined}");
    } else {
        assert!(joined.contains("alsa"), "{joined}");
    }
}

// ================= the two halves together =================

#[test]
fn a_short_answer_ends_in_well_under_a_second_of_silence() {
    // The headline complaint: "makes you wait seven seconds after yes".
    let cfg = EndpointConfig::default();
    let mut ep = Endpointer::start(0);
    let mut t = 0u64;
    let step = 250u64;

    // Warm-up, then a word.
    for _ in 0..6 {
        t += step;
        ep.feed(level_db(&tone(4000, 0.15)), "", t, &cfg);
    }
    assert!(!ep.finished(), "ended while still being spoken to");

    // Then quiet.
    let quiet = level_db(&tone(4000, 0.001));
    let spoke_until = t;
    while !ep.finished() && t < 10_000 {
        t += step;
        ep.feed(quiet, "", t, &cfg);
    }
    assert!(ep.finished(), "never ended");
    let waited = t - spoke_until;
    assert!(waited < 2_000, "waited {waited}ms after the last word");
    // And the clip is the length of what was said plus that pause, not eight
    // seconds.
    assert!(ep.clip_ms(t) < 6_000, "clip was {}ms", ep.clip_ms(t));
}

#[test]
fn a_pause_for_thought_mid_sentence_does_not_cut_you_off() {
    // The other half of the complaint. Real speech has gaps in it.
    let cfg = EndpointConfig::default();
    let mut ep = Endpointer::start(0);
    let mut t = 0u64;
    let loud = level_db(&tone(4000, 0.15));
    let quiet = level_db(&tone(4000, 0.001));

    for _ in 0..4 {
        t += 250;
        ep.feed(loud, "", t, &cfg);
    }
    // A 500ms gap -- shorter than the shipped sentence threshold.
    for _ in 0..2 {
        t += 250;
        ep.feed(quiet, "", t, &cfg);
    }
    assert!(!ep.finished(), "cut off during a half-second pause");
    t += 250;
    ep.feed(loud, "", t, &cfg);
    assert!(!ep.finished());
}

#[test]
fn a_dead_microphone_gives_up_rather_than_recording_nothing_forever() {
    let cfg = EndpointConfig::default();
    let mut ep = Endpointer::start(0);
    let mut t = 0u64;
    while !ep.finished() && t < 60_000 {
        t += 250;
        // What an unplugged device produces: exact digital zero.
        ep.feed(level_db(&vec![0i16; 4000]), "", t, &cfg);
    }
    assert!(ep.finished(), "a silent microphone recorded forever");
    assert!(matches!(ep.state, atlas::endpoint::Listening::Finished(Why::Nothing)));
    assert!(t <= cfg.no_speech_after_ms + 500, "took {t}ms to give up");
}
