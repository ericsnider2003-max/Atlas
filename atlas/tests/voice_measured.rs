//! Round 5: the in-house voice pieces, measured on synthesized speakers.
//!
//! Six espeak-ng voices (tests/fixtures/speech/corpus, made by
//! `make_corpus.py`), each saying ten different sentences at a varied pitch
//! and speed, plus "hey atlas" nine times and six near-misses in two of them.
//! Synthesized voices are more consistent than people, so these numbers are
//! the ceiling of what to expect, not the floor; the tests say so where it
//! matters. Its own target so it runs optimised:
//!   cargo test --release --test voice_measured -- --nocapture

mod common; // `common::source_of`: a module's source wherever its files live

use atlas::speaker::{clip_frames, Background};
use atlas::voiceid::{cosine, VoiceIdConfig};

fn wav(name: &str) -> Vec<i16> {
    let (s, r) = atlas::diarize::read_wav(&std::fs::read(format!("tests/fixtures/speech/corpus/{name}.wav")).unwrap()).unwrap();
    assert_eq!(r, 16000);
    s
}

const SPEAKERS: [&str; 6] = ["A", "B", "C", "D", "E", "F"];

/// A fan: low-passed noise at `snr_db` under the clip's own level.
fn with_fan(s: &[i16], snr_db: f32, seed: u64) -> Vec<i16> {
    let mut x = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    let mut lp = 0f32;
    let noise: Vec<f32> = (0..s.len())
        .map(|_| {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let w = ((x >> 33) as f32 / (1u64 << 31) as f32) - 1.0;
            lp = 0.9 * lp + 0.1 * w;
            lp
        })
        .collect();
    let sp = (s.iter().map(|v| (*v as f32).powi(2)).sum::<f32>() / s.len() as f32).sqrt();
    let np = (noise.iter().map(|v| v * v).sum::<f32>() / noise.len() as f32).sqrt();
    let k = sp / np / 10f32.powf(snr_db / 20.0);
    s.iter().zip(noise).map(|(a, n)| (*a as f32 + n * k).clamp(-32767.0, 32767.0) as i16).collect()
}

fn pad(s: &[i16]) -> Vec<i16> {
    let mut v = vec![0i16; 8000];
    v.extend_from_slice(s);
    v.extend(vec![0i16; 8000]);
    v
}

fn q(v: &mut Vec<f32>, p: f32) -> f32 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[((v.len() - 1) as f32 * p) as usize]
}

#[test]
fn builtin_encoder_thresholds_are_where_the_voices_put_them() {
    let frames: Vec<(usize, Vec<[f32; 19]>)> = SPEAKERS
        .iter()
        .enumerate()
        .flat_map(|(k, s)| (0..10).map(move |i| (k, clip_frames(&wav(&format!("{s}_s{i}")), 16000).unwrap())))
        .collect();
    // The machine's background: every clip it has heard (all six voices).
    let bg = Background::of(&frames.iter().map(|c| c.1.clone()).collect::<Vec<_>>());
    assert!(bg.ready());
    let z: Vec<(usize, Vec<f32>)> = frames.iter().map(|(k, f)| (*k, bg.embed(f).unwrap())).collect();
    let (mut own, mut others) = (vec![], vec![]);
    let cfg = VoiceIdConfig::default();
    let mut verdicts = (0, 0, 0); // own accepted, own rejected, others accepted
    for k in 0..SPEAKERS.len() {
        let mut id = atlas::voiceid::VoiceId::default();
        for e in z.iter().filter(|c| c.0 == k).take(5) {
            id.enroll(&e.1).unwrap();
        }
        let cen = id.print.as_ref().unwrap().centroid.clone();
        for (i, c) in z.iter().enumerate() {
            let enrolled = c.0 == k && i % 10 < 5;
            if enrolled {
                continue;
            }
            let s = cosine(&c.1, &cen);
            let v = id.check(&c.1, &VoiceIdConfig { min_samples: 3, ..cfg.clone() });
            if c.0 == k {
                own.push(s);
                verdicts.0 += matches!(v, atlas::voiceid::Verdict::You(_)) as usize;
                verdicts.1 += matches!(v, atlas::voiceid::Verdict::NotYou(_)) as usize;
            } else {
                others.push(s);
                verdicts.2 += matches!(v, atlas::voiceid::Verdict::You(_)) as usize;
            }
        }
    }
    let (o2, x98) = (q(&mut own.clone(), 0.02), q(&mut others.clone(), 0.98));
    println!(
        "LIVE [voice-lock, built-in encoder]  6 voices, 5 enrolled each, {} own and {} other clips scored",
        own.len(),
        others.len()
    );
    println!(
        "  own: mean {:.2}, 2nd percentile {o2:.2};  others: mean {:.2}, 98th percentile {x98:.2}",
        own.iter().sum::<f32>() / own.len() as f32,
        others.iter().sum::<f32>() / others.len() as f32
    );
    println!(
        "  with the shipped lines ({} / {}): own heard as you {}/{}, own taken for someone else {}/{}, others taken for you {}/{}",
        cfg.builtin_accept,
        cfg.builtin_reject,
        verdicts.0,
        own.len(),
        verdicts.1,
        own.len(),
        verdicts.2,
        others.len()
    );
    // "You" starts where only 2% of other voices reach; "not you" sits below
    // the lowest 2% of your own.
    assert!((cfg.builtin_accept - x98).abs() <= 0.03, "accept {} vs measured {x98:.3}", cfg.builtin_accept);
    assert!(cfg.builtin_reject < o2 && cfg.builtin_reject < cfg.builtin_accept, "reject {} vs own 2nd percentile {o2:.3}", cfg.builtin_reject);
    assert!(verdicts.2 as f32 / others.len() as f32 <= 0.03);
    assert!(verdicts.1 as f32 / own.len() as f32 <= 0.03);
}

#[test]
fn your_wake_phrase_from_three_takes_is_heard_and_near_misses_are_not() {
    let takes: Vec<_> = (0..3).map(|i| atlas::wakeword::phrase_take(&wav(&format!("A_wake{i}")), 16000).unwrap()).collect();
    let m = atlas::wakeword::train(takes).unwrap();
    let hit = |s: &[i16]| atlas::wakeword::heard(s, 16000, &m);
    let (mut clean, mut fan) = (0, 0);
    for i in 3..9 {
        let s = pad(&wav(&format!("A_wake{i}")));
        clean += hit(&s) as usize;
        fan += hit(&with_fan(&s, 10.0, i)) as usize;
    }
    let other_voice = (0..9).filter(|i| hit(&pad(&wav(&format!("B_wake{i}"))))).count();
    let near = ["A", "B"].iter().flat_map(|s| (0..6).map(move |i| format!("{s}_near{i}"))).filter(|n| hit(&pad(&wav(n)))).count();
    let sentences = ["A", "B", "C"].iter().flat_map(|s| (0..10).map(move |i| format!("{s}_s{i}"))).filter(|n| hit(&wav(n))).count();
    println!("LIVE [wake word, DTW on three takes]  threshold {:.2}", m.threshold);
    println!("  your phrase: {clean}/6 clean, {fan}/6 with a fan at 10 dB");
    println!("  false wakes: {near}/12 near-misses (\"hey alice\", \"hey at last\", \"at least\"…), {sentences}/30 sentences");
    println!("  another voice saying \"hey atlas\": {other_voice}/9 — matched to your voice, by design");
    assert_eq!((clean, fan), (6, 6));
    assert_eq!((near, sentences), (0, 0));
}

#[test]
fn hearing_calibrated_to_a_room_scores_at_least_as_well_as_the_shipped_defaults() {
    // You, somewhere quiet: four sentences in a row.
    let mut you = Vec::new();
    for i in 0..4 {
        you.extend(wav(&format!("A_s{i}")));
        you.extend(vec![0i16; 6000]);
    }
    // The room: a loud fan, at the level it would reach the same microphone.
    let loud = (you.iter().map(|v| (*v as f32).powi(2)).sum::<f32>() / you.len() as f32).sqrt();
    // Fan noise alone: `with_fan` on a clip of constant level gives noise of
    // that level (plus a constant, which the mic's DC blocking would remove).
    let base = vec![1000i16; 16000 * 8];
    let room: Vec<i16> = with_fan(&base, 0.0, 9).iter().map(|v| v - 1000).collect();
    let rms = (room.iter().map(|v| (*v as f32).powi(2)).sum::<f32>() / room.len() as f32).sqrt().max(1.0);
    let room: Vec<i16> = room.iter().map(|v| (*v as f32 * loud / rms / 10f32.powf(6.0 / 20.0)) as i16).collect();
    let c = atlas::vadcal::calibrate(&room, &you, 16000, atlas::vad::DEFAULT_MEASURED).unwrap();
    println!("LIVE [hearing calibration, a fan 6 dB under your voice]  {}", c.say());
    assert!(c.best_score >= c.before_score);
    assert!(c.best_score > 0.8, "{}", c.best_score);
}

#[test]
fn a_three_way_call_is_split_by_voice_with_the_builtin_encoder() {
    // A, B, A, C, B, A — six turns, 0.8 s apart, a fan at 15 dB.
    let order = [("A", 0), ("B", 1), ("A", 2), ("C", 3), ("B", 4), ("A", 5)];
    let mut call = Vec::new();
    let mut truth = Vec::new();
    for (spk, i) in order {
        call.extend(vec![0i16; 12800]);
        let s = wav(&format!("{spk}_s{i}"));
        truth.push((call.len(), call.len() + s.len(), spk));
        call.extend(s);
    }
    call.extend(vec![0i16; 12800]);
    let call = with_fan(&call, 15.0, 3);
    // A machine that has heard you before (four other sentences of yours) and
    // three other voices — not B or C, who are new on this call.
    let heard: Vec<Vec<[f32; 19]>> = ["D", "E", "F"]
        .iter()
        .flat_map(|s| (0..10).map(move |i| format!("{s}_s{i}")))
        .chain((6..10).map(|i| format!("A_s{i}")))
        .map(|n| clip_frames(&wav(&n), 16000).unwrap())
        .collect();
    let machine = Background::of(&heard);
    let own: Vec<Option<Vec<f32>>> = atlas::speaker::recording_embeddings(&call, 16000, &machine).into_iter().map(|(_, e)| e).collect();
    let mut next = own.into_iter();
    let mut embed = |_: &[i16]| next.next().flatten();
    let mut hear = |_: &[i16]| None;
    let lines = atlas::diarize::who_said_what_grouped(&call, 16000, &mut embed, &mut hear, None, VoiceIdConfig::default().builtin_accept, atlas::speaker::GROUP_CENTRED_AT);
    // Each line's true speaker is the turn it overlaps most.
    let mut pairs = Vec::new();
    for l in &lines {
        let (s, e) = (l.start_ms as usize * 16, l.end_ms as usize * 16);
        let t = truth.iter().max_by_key(|(a, b, _)| (*b).min(e).saturating_sub((*a).max(s))).unwrap().2;
        pairs.push((l.speaker.clone(), t));
    }
    // Best one-to-one naming of the found speakers against the real ones.
    let found: Vec<String> = { let mut v: Vec<String> = pairs.iter().map(|p| p.0.clone()).collect(); v.sort(); v.dedup(); v };
    let mut right = 0;
    for l in &found {
        let mut counts = std::collections::BTreeMap::new();
        for p in pairs.iter().filter(|p| &p.0 == l) {
            *counts.entry(p.1).or_insert(0) += 1;
        }
        right += counts.values().max().copied().unwrap_or(0);
    }
    println!("LIVE [notes, built-in encoder, 3 voices + fan]  {} lines, {} speakers found, {right}/{} on a consistent speaker", lines.len(), found.len(), pairs.len());
    for l in &lines {
        println!("    {}", l.say());
    }
    // Measured: every line lands on a label that is one real speaker (no
    // label mixes two people). It can over-split — a turn whose words differ
    // most from the same speaker's others may get a label of its own — which
    // is the safer mistake for notes: nothing is attributed to the wrong
    // person.
    assert!((3..=4).contains(&found.len()), "{found:?}");
    assert_eq!(right, pairs.len(), "a label mixed two speakers");
}

/// One synthetic call: `order` is (speaker, sentence) turns, 0.8 s apart,
/// with a fan at 15 dB. The machine has heard the voices not on the call.
/// Returns the call, the truth per turn, and the built-in grouping's lines.
fn a_call(order: &[(&str, usize)], seed: u64) -> (Vec<i16>, Vec<(usize, usize, String)>, Vec<atlas::diarize::Line>) {
    let mut call = Vec::new();
    let mut truth = Vec::new();
    for (spk, i) in order {
        call.extend(vec![0i16; 12800]);
        let s = wav(&format!("{spk}_s{i}"));
        truth.push((call.len(), call.len() + s.len(), spk.to_string()));
        call.extend(s);
    }
    call.extend(vec![0i16; 12800]);
    let call = with_fan(&call, 15.0, seed);
    let heard: Vec<Vec<[f32; 19]>> = SPEAKERS
        .iter()
        .filter(|s| !order.iter().any(|o| o.0 == **s))
        .flat_map(|s| (0..10).map(move |i| format!("{s}_s{i}")))
        .map(|n| clip_frames(&wav(&n), 16000).unwrap())
        .collect();
    let machine = Background::of(&heard);
    let own: Vec<Option<Vec<f32>>> = atlas::speaker::recording_embeddings(&call, 16000, &machine).into_iter().map(|(_, e)| e).collect();
    let mut next = own.into_iter();
    let mut embed = |_: &[i16]| next.next().flatten();
    let mut hear = |_: &[i16]| None;
    let lines = atlas::diarize::who_said_what_grouped(&call, 16000, &mut embed, &mut hear, None, VoiceIdConfig::default().builtin_accept, atlas::speaker::GROUP_CENTRED_AT);
    (call, truth, lines)
}

/// (labels found, lines on a label that is one real speaker, lines).
fn score(lines: &[atlas::diarize::Line], truth: &[(usize, usize, String)]) -> (usize, usize, usize) {
    let mut pairs = Vec::new();
    for l in lines {
        let (s, e) = (l.start_ms as usize * 16, l.end_ms as usize * 16);
        let t = truth.iter().max_by_key(|(a, b, _)| (*b).min(e).saturating_sub((*a).max(s))).unwrap().2.clone();
        pairs.push((l.speaker.clone(), t));
    }
    let mut found: Vec<String> = pairs.iter().map(|p| p.0.clone()).collect();
    found.sort();
    found.dedup();
    let mut right = 0;
    for l in &found {
        let mut counts = std::collections::BTreeMap::new();
        for p in pairs.iter().filter(|p| &p.0 == l) {
            *counts.entry(p.1.clone()).or_insert(0) += 1;
        }
        right += counts.values().max().copied().unwrap_or(0);
    }
    (found.len(), right, pairs.len())
}

/// Sixty calls: two or three of the six voices, six to eight turns,
/// each voice at least twice, in shuffled order.
fn the_calls() -> Vec<Vec<(&'static str, usize)>> {
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    let mut rnd = |n: usize| {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((x >> 33) as usize) % n
    };
    let mut calls = Vec::new();
    for c in 0..60 {
        let people = if c % 3 == 0 { 2 } else { 3 };
        let mut who: Vec<&'static str> = Vec::new();
        while who.len() < people {
            let s = SPEAKERS[rnd(6)];
            if !who.contains(&s) {
                who.push(s);
            }
        }
        let turns = 6 + rnd(3);
        let mut order: Vec<&'static str> = who.iter().flat_map(|s| [*s, *s]).collect();
        while order.len() < turns {
            order.push(who[rnd(people)]);
        }
        for i in (1..order.len()).rev() {
            order.swap(i, rnd(i + 1));
        }
        let mut used: Vec<(&'static str, usize)> = Vec::new();
        for s in order {
            let mut k = rnd(10);
            while used.contains(&(s, k)) {
                k = (k + 1) % 10;
            }
            used.push((s, k));
        }
        calls.push(used);
    }
    calls
}

// Slow in a debug build, not stuck (measured 28 Sep 2026 on the two-CPU
// box, debug, one at a time: 258 s). The target as a whole ran past ten
// minutes in round 3 for that reason alone. Ignored in the ordinary run;
// `a_three_way_call_is_split_by_voice_with_the_builtin_encoder` (2 s) keeps
// the grouping covered there. Run them optimised, as this file's header says:
//   cargo test --release --test voice_measured -- --ignored --nocapture
#[test]
#[ignore = "about 4 minutes in a debug build; run with --release -- --ignored"]
fn a_second_look_on_pooled_speech_merges_a_split_voice_without_mixing_two() {
    // Sixty calls. The penalty is chosen on the first forty and then checked
    // on the last twenty, which played no part in choosing it.
    let calls = the_calls();
    let lambdas = [1.0, 1.1, 1.2, 1.3, 1.4, 1.5, 1.7, 2.0];
    // Per setting and half: [right count, over-split, mixed, pure lines, lines].
    let mut table = vec![[[0usize; 5]; 2]; lambdas.len() + 1];
    for (c, order) in calls.iter().enumerate() {
        let half = if c < 40 { 0 } else { 1 };
        let (call, truth, lines) = a_call(order, c as u64 + 1);
        let people = { let mut v: Vec<&str> = order.iter().map(|o| o.0).collect(); v.sort(); v.dedup(); v.len() };
        let mut tally = |k: usize, lines: &[atlas::diarize::Line]| {
            let (found, right, n) = score(lines, &truth);
            let row = &mut table[k][half];
            if right < n {
                row[2] += 1;
            } else if found == people {
                row[0] += 1;
            } else if found > people {
                row[1] += 1;
            }
            row[3] += right;
            row[4] += n;
        };
        tally(0, &lines);
        for (k, l) in lambdas.iter().enumerate() {
            let merged = atlas::diarize::merge_same_voices(&call, 16000, lines.clone(), *l);
            tally(k + 1, &merged);
        }
    }
    for (half, name) in [(0, "the 40 calls the penalty is chosen on"), (1, "the 20 held-out calls")] {
        println!("LIVE [who said what, 2-3 voices + fan — {name}]");
        println!("    {:<14} {:>6} {:>11} {:>7} {:>12}", "setting", "right", "over-split", "mixed", "pure lines");
        for (k, row) in table.iter().enumerate() {
            let label = if k == 0 { "grouping only".to_string() } else { format!("λ = {}", lambdas[k - 1]) };
            let r = row[half];
            println!("    {:<14} {:>6} {:>11} {:>7} {:>7}/{}", label, r[0], r[1], r[2], r[3], r[4]);
        }
    }
    // The rule: most calls exactly right without mixing more than grouping
    // alone does; on a tie, the gentlest penalty.
    let base = table[0][0];
    let mut pick = 0;
    for k in 1..table.len() {
        let r = table[k][0];
        if r[2] <= base[2] && r[0] > table[pick][0][0] {
            pick = k;
        }
    }
    assert!(pick > 0, "no penalty did better than grouping alone on the choosing calls");
    assert_eq!(lambdas[pick - 1], atlas::diarize::SAME_VOICE_LAMBDA, "the shipped penalty isn't the one the calls pick");
    let (held_base, held) = (table[0][1], table[pick][1]);
    println!(
        "LIVE [who said what] chosen λ = {}: on the held-out calls, over-split {} → {}, mixed {} → {}",
        lambdas[pick - 1], held_base[1], held[1], held_base[2], held[2]
    );
    // What the held-out calls say decides whether the second look is on by
    // default: it is only if it never mixed two people there. Measured, it
    // did (0 → 1 of 20), so `atlas notes` takes it only with --merge-voices.
    let safe_by_default = held[2] <= held_base[2];
    let main = crate::common::source_of("main");
    assert_eq!(
        main.contains("let lines = if merge_voices {"),
        !safe_by_default,
        "the second look's default in `atlas notes` doesn't match what the held-out calls say"
    );
    assert!(held[1] <= held_base[1], "it should never split more than grouping alone");
}

/// Sixty calls like `the_calls`, except that in every other one a further
/// voice speaks exactly once — a real one-turn speaker, who must keep a
/// label of their own.
fn the_calls_with_loners() -> Vec<Vec<(&'static str, usize)>> {
    let mut x = 0x2545_F491_4F6C_DD1Du64;
    let mut rnd = |n: usize| {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((x >> 33) as usize) % n
    };
    let mut calls = the_calls();
    for (c, call) in calls.iter_mut().enumerate() {
        if c % 2 == 1 {
            let mut s = SPEAKERS[rnd(6)];
            while call.iter().any(|t| t.0 == s) {
                s = SPEAKERS[rnd(6)];
            }
            let at = 1 + rnd(call.len() - 1);
            call.insert(at, (s, rnd(10)));
        }
    }
    calls
}

// Slow in a debug build, not stuck (measured 28 Sep 2026 on the two-CPU
// box, debug, one at a time: 467 s). The target as a whole ran past ten
// minutes in round 3 for that reason alone. Ignored in the ordinary run;
// `a_three_way_call_is_split_by_voice_with_the_builtin_encoder` (2 s) keeps
// the grouping covered there. Run them optimised, as this file's header says:
//   cargo test --release --test voice_measured -- --ignored --nocapture
#[test]
#[ignore = "about 8 minutes in a debug build; run with --release -- --ignored"]
fn someone_who_speaks_once_gets_their_own_name_back() {
    // Sixty calls in which every other one has a person who speaks once.
    // The margin is chosen on the first forty and checked on the last twenty.
    let calls = the_calls_with_loners();
    let margins = [1.0, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0];
    // Per setting and half: [right, over-split, mixed, pure lines, lines].
    let mut table = vec![[[0usize; 5]; 2]; margins.len() + 1];
    for (c, order) in calls.iter().enumerate() {
        let half = if c < 40 { 0 } else { 1 };
        let (call, truth, lines) = a_call(order, c as u64 + 101);
        let people = {
            let mut v: Vec<&str> = order.iter().map(|o| o.0).collect();
            v.sort();
            v.dedup();
            v.len()
        };
        let mut tally = |k: usize, lines: &[atlas::diarize::Line]| {
            let (found, right, n) = score(lines, &truth);
            let row = &mut table[k][half];
            if right < n {
                row[2] += 1;
            } else if found == people {
                row[0] += 1;
            } else if found > people {
                row[1] += 1;
            }
            row[3] += right;
            row[4] += n;
        };
        tally(0, &lines);
        for (k, m) in margins.iter().enumerate() {
            tally(k + 1, &atlas::diarize::split_strangers(&call, 16000, lines.clone(), *m));
        }
    }
    for (half, name) in [(0, "the 40 calls the margin is chosen on"), (1, "the 20 held-out calls")] {
        println!("LIVE [someone speaks once, 2-4 voices + fan — {name}]");
        println!("    {:<14} {:>6} {:>11} {:>7} {:>12}", "setting", "right", "over-split", "mixed", "pure lines");
        for (k, row) in table.iter().enumerate() {
            let label = if k == 0 { "grouping only".to_string() } else { format!("margin {}", margins[k - 1]) };
            let r = row[half];
            println!("    {:<14} {:>6} {:>11} {:>7} {:>7}/{}", label, r[0], r[1], r[2], r[3], r[4]);
        }
    }
    // The rule: most calls exactly right; on a tie, the larger margin (the
    // gentler one).
    let mut pick = 0;
    for k in 1..table.len() {
        if table[k][0][0] >= table[pick][0][0] {
            pick = k;
        }
    }
    assert!(pick > 0, "splitting did no better than grouping alone on the choosing calls");
    assert_eq!(margins[pick - 1], atlas::diarize::STRANGER_MARGIN, "the shipped margin isn't the one the calls pick");
    let (base, held) = (table[0][1], table[pick][1]);
    println!(
        "LIVE [someone speaks once] chosen margin {}: held out, right {} → {}, mixed {} → {}, over-split {} → {}",
        margins[pick - 1], base[0], held[0], base[2], held[2], base[1], held[1]
    );
    // On by default only if, on calls it wasn't chosen on, it mixed no more
    // people and split no more voices than grouping alone.
    let safe_by_default = held[2] <= base[2] && held[1] <= base[1];
    let main = crate::common::source_of("main");
    assert_eq!(
        main.contains("let lines = atlas::diarize::split_strangers(&samples, rate, lines, atlas::diarize::STRANGER_MARGIN);"),
        safe_by_default,
        "whether `atlas notes` splits strangers by default doesn't match what the held-out calls say"
    );
    // And on the sixty calls where everyone speaks at least twice, it
    // mustn't cost more than it saves.
    let (mut before, mut after) = ([0usize; 3], [0usize; 3]);
    for (c, order) in the_calls().iter().enumerate() {
        let (call, truth, lines) = a_call(order, c as u64 + 1);
        let people = {
            let mut v: Vec<&str> = order.iter().map(|o| o.0).collect();
            v.sort();
            v.dedup();
            v.len()
        };
        for (row, l) in [(&mut before, lines.clone()), (&mut after, atlas::diarize::split_strangers(&call, 16000, lines, atlas::diarize::STRANGER_MARGIN))] {
            let (found, right, n) = score(&l, &truth);
            if right < n {
                row[2] += 1;
            } else if found == people {
                row[0] += 1;
            } else {
                row[1] += 1;
            }
        }
    }
    println!(
        "LIVE [someone speaks once] on the 60 calls where everyone speaks twice or more: right {} → {}, over-split {} → {}, mixed {} → {}",
        before[0], after[0], before[1], after[1], before[2], after[2]
    );
    assert!(after[2] <= before[2] && after[0] + 3 >= before[0], "{before:?} → {after:?}");
}

// Slow in a debug build, not stuck (measured 28 Sep 2026 on the two-CPU
// box, debug, one at a time: 371 s). The target as a whole ran past ten
// minutes in round 3 for that reason alone. Ignored in the ordinary run;
// `a_three_way_call_is_split_by_voice_with_the_builtin_encoder` (2 s) keeps
// the grouping covered there. Run them optimised, as this file's header says:
//   cargo test --release --test voice_measured -- --ignored --nocapture
#[test]
#[ignore = "about 6 minutes in a debug build; run with --release -- --ignored"]
fn told_how_many_people_there_were_the_grouping_gets_it_right_more_often() {
    // Every call from both sets, run the way `atlas notes --people N` runs:
    // grouping, strangers split, then brought to the count.
    let mut rows = [[0usize; 3]; 2];
    let sets = [the_calls(), the_calls_with_loners()];
    for (set, calls) in sets.iter().enumerate() {
        for (c, order) in calls.iter().enumerate() {
            let seed = if set == 0 { c as u64 + 1 } else { c as u64 + 101 };
            let (call, truth, lines) = a_call(order, seed);
            let people = {
                let mut v: Vec<&str> = order.iter().map(|o| o.0).collect();
                v.sort();
                v.dedup();
                v.len()
            };
            let split = atlas::diarize::split_strangers(&call, 16000, lines.clone(), atlas::diarize::STRANGER_MARGIN);
            let counted = atlas::diarize::to_count(&call, 16000, split.clone(), people);
            for (k, l) in [(0, &split), (1, &counted)] {
                let (found, right, n) = score(l, &truth);
                if right < n {
                    rows[k][2] += 1;
                } else if found == people {
                    rows[k][0] += 1;
                } else {
                    rows[k][1] += 1;
                }
            }
            assert_eq!(score(&counted, &truth).0, people, "told {people}, it should say {people}");
        }
    }
    println!(
        "LIVE [told the count] 120 calls: exactly right {} → {}, over-split {} → {}, mixed {} → {}",
        rows[0][0], rows[1][0], rows[0][1], rows[1][1], rows[0][2], rows[1][2]
    );
    assert!(rows[1][0] > rows[0][0], "{rows:?}");
}
