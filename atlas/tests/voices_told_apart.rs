//! The trained speaker model (`speakernet`, CAM++), checked against the
//! reference tools it was measured with (kaldi-native-fbank and
//! onnxruntime, 30 Sep 2026). Fixtures: four LibriSpeech test-clean
//! utterances (CC-BY 4.0), two speakers, the first 4 s of each.

fn wav(name: &str) -> Vec<f32> {
    let bytes = std::fs::read(format!("tests/fixtures/voices/{name}")).unwrap();
    let (s, rate) = atlas::diarize::read_wav(&bytes).unwrap();
    assert_eq!(rate, 16000);
    s.iter().map(|v| *v as f32 / 32768.0).collect()
}

#[test]
fn the_filterbank_matches_kaldis() {
    let reference: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string("tests/fixtures/voices/fbank_reference.json").unwrap()).unwrap();
    for (name, r) in reference.as_object().unwrap() {
        let f = atlas::speakernet::fbank(&wav(name));
        assert_eq!(f.len() as u64, r["frames"].as_u64().unwrap(), "{name}: frame count");
        for (row, key) in [(0usize, "f0"), (100, "f100")] {
            let want: Vec<f32> = r[key].as_array().unwrap().iter().map(|v| v.as_f64().unwrap() as f32).collect();
            let worst = f[row].iter().zip(&want).map(|(a, b)| (a - b).abs()).fold(0f32, f32::max);
            assert!(worst < 0.02, "{name} frame {row}: off by {worst}");
        }
    }
}

/// With the model present (`ATLAS_VOICE_MODEL` pointing at
/// campplus_en_voxceleb.onnx): the same voice scores far above a different
/// one, as it did in onnxruntime (0.75 and 0.77 same, 0.18-0.25 different).
#[test]
fn the_same_voice_scores_above_a_different_one() {
    let Ok(model) = std::env::var("ATLAS_VOICE_MODEL") else { return };
    let dir = std::path::Path::new(&model).parent().unwrap().to_path_buf();
    let file = std::path::Path::new(&model).file_name().unwrap().to_string_lossy().to_string();
    assert_eq!(file, atlas::speakernet::FILE);
    let e = |n: &str| atlas::speakernet::embed(&wav(n), &dir).unwrap();
    let (a0, a1, b0, b1) = (e("1320-0.wav"), e("1320-1.wav"), e("260-0.wav"), e("260-1.wav"));
    let cos = |x: &[f32], y: &[f32]| x.iter().zip(y).map(|(a, b)| a * b).sum::<f32>();
    let same = [cos(&a0, &a1), cos(&b0, &b1)];
    let diff = [cos(&a0, &b0), cos(&a0, &b1), cos(&a1, &b0), cos(&a1, &b1)];
    eprintln!("same {same:?} different {diff:?}");
    assert!((same[0] - 0.7504).abs() < 0.03 && (same[1] - 0.7724).abs() < 0.03, "{same:?}");
    assert!(diff.iter().all(|d| *d < 0.3), "{diff:?}");
}

/// Each encoder is judged by its own lines.
#[test]
fn each_encoder_has_its_own_lines() {
    let c = atlas::voiceid::VoiceIdConfig::default();
    assert_eq!(c.lines_for(atlas::speakernet::DIMS), (0.45, 0.25));
    assert_eq!(c.lines_for(atlas::speaker::BUILTIN_DIMS), (0.30, 0.15));
    assert_eq!(c.lines_for(192), (c.accept, c.reject));
}

/// `atlas get voiceid` fetches the model `speakernet` looks for, pinned.
#[test]
fn the_voice_model_is_fetched_by_name_and_pinned() {
    let pieces = atlas::getpieces::voice_model();
    assert_eq!(pieces.len(), 1);
    assert_eq!(pieces[0].key_path(), format!("models/{}", atlas::speakernet::FILE));
    assert_eq!(pieces[0].bytes, 29_596_978);
    let (_, set) = atlas::getpieces::set(Some("voiceid")).unwrap();
    assert_eq!(set[0].sha256, pieces[0].sha256);
}
