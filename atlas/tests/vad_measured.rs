//! The speech detector's thresholds, measured (round 4). Its own target so
//! it can run optimised: the grid is ~650 settings over five 20-second rooms.
//!   cargo test --release --test vad_measured -- --nocapture

// ---- vad: thresholds measured on real (synthesized) speech, round-3 gap -------------------

fn speech_fixture(room: &str) -> (Vec<i16>, u32) {
    atlas::diarize::read_wav(&std::fs::read(format!("tests/fixtures/speech/call_{room}.wav")).unwrap()).unwrap()
}

fn truth() -> (Vec<bool>, Vec<(usize, usize, String)>) {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string("tests/fixtures/speech/truth.json").unwrap()).unwrap();
    let t = v["truth"].as_array().unwrap().iter().map(|x| x.as_i64() == Some(1)).collect();
    let turns = v["turns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| (x[0].as_u64().unwrap() as usize, x[1].as_u64().unwrap() as usize, x[2].as_str().unwrap().to_string()))
        .collect();
    (t, turns)
}

const ROOMS: [&str; 5] = ["quiet", "fan10", "fan5", "aircon5", "hum10"];

/// Mean over rooms of balanced accuracy: (speech frames heard + silent
/// frames left alone) / 2.
fn score(p: atlas::vad::VadParams, audio: &[(Vec<i16>, u32)], truth: &[bool]) -> (f64, Vec<(f64, f64)>) {
    let mut per = Vec::new();
    for (samples, rate) in audio {
        // The same path the live microphone takes: a tuned detector fed one
        // 10 ms frame at a time through `window`.
        let mut v = atlas::vad::Vad::tuned(*rate, p);
        let frame = (*rate as usize / 100).max(1);
        let f: Vec<bool> = samples.chunks(frame).filter(|c| c.len() == frame).map(|c| v.window(c).map(|s| s > 0.5).unwrap_or(false)).collect();
        let n = f.len().min(truth.len());
        let (mut tp, mut sp, mut tn, mut sn) = (0.0, 0.0, 0.0, 0.0);
        for i in 30..n {
            if truth[i] {
                sp += 1.0;
                tp += f[i] as u8 as f64;
            } else {
                sn += 1.0;
                tn += (!f[i]) as u8 as f64;
            }
        }
        per.push((tp / sp, tn / sn));
    }
    let mean = per.iter().map(|(a, b)| (a + b) / 2.0).sum::<f64>() / per.len() as f64;
    (mean, per)
}

#[test]
fn vad_thresholds_are_the_measured_best() {
    let audio: Vec<(Vec<i16>, u32)> = ROOMS.iter().map(|r| speech_fixture(r)).collect();
    let (t, _) = truth();
    let mut best = (0.0, atlas::vad::DEFAULT_MEASURED);
    for e in [0.5, 1.0, 1.5, 2.0, 3.0, 4.0, 6.0, 8.0, 10.0] {
        for fl in [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 8.0] {
            for loud in [4.0, 6.0, 8.0, 10.0, 12.0, 16.0, 20.0] {
                let p = atlas::vad::VadParams { energy_db: e, flatness_db: fl, loud_db: loud };
                let (s, _) = score(p, &audio, &t);
                if s > best.0 + 1e-9 {
                    best = (s, p);
                }
            }
        }
    }
    let (shipped, per) = score(atlas::vad::DEFAULT_MEASURED, &audio, &t);
    println!("LIVE [vad measured]  grid best {:.4} at {:?}", best.0, best.1);
    println!("  shipped {:?}: {shipped:.4}", atlas::vad::DEFAULT_MEASURED);
    for (room, (hit, quiet)) in ROOMS.iter().zip(&per) {
        println!("    {room:<8} speech heard {:.1}%   silence left alone {:.1}%", 100.0 * hit, 100.0 * quiet);
    }
    assert!(shipped >= best.0 - 0.005, "the shipped thresholds are not the measured best: {shipped:.4} vs {:.4} at {:?}", best.0, best.1);
    assert!(shipped > 0.84, "{shipped}");
}

