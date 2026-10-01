//! The wake-word spotter (`kws`, 1 Oct 2026): sherpa-onnx's keyword
//! spotting, run in Atlas's process. Fixtures: four piper-synthesised
//! clips, 16 kHz -- two with the name, two near-misses.

fn wav(name: &str) -> Vec<i16> {
    let bytes = std::fs::read(format!("tests/fixtures/wake/{name}.wav")).unwrap();
    let (s, rate) = atlas::diarize::read_wav(&bytes).unwrap();
    assert_eq!(rate, 16000);
    s
}

/// With the library and model present (`ATLAS_KWS_ROOT`, an install folder
/// with `tools/sherpa/lib` and `models/kws`): the name is heard in the two
/// that say it -- including one Parakeet wrote as "Alice, what time is
/// it?" -- and not in "Alice, can you pass the salt?" or "Hey Alexa".
#[test]
fn the_name_is_heard_by_its_sound() {
    let Ok(root) = std::env::var("ATLAS_KWS_ROOT") else { return };
    let root = std::path::PathBuf::from(root);
    for (clip, want) in [("hey-atlas-open-chrome", true), ("atlas-what-time-misheard", true), ("alice-pass-the-salt", false), ("hey-alexa", false)] {
        assert_eq!(atlas::kws::heard_name(&root, &wav(clip)), Some(want), "{clip}");
    }
}

/// `atlas get wakeword` fetches it, pinned, and `atlas get hearing` brings
/// it with Parakeet.
#[test]
fn the_spotter_is_fetched_by_name_and_pinned() {
    let (_, pieces) = atlas::getpieces::set(Some("wakeword")).unwrap();
    assert_eq!(pieces.len(), 1);
    assert_eq!(pieces[0].sha256.len(), 64);
    assert!(pieces[0].url.contains("kws-models"));
    let (_, hearing) = atlas::getpieces::set(Some("hearing")).unwrap();
    assert!(hearing.iter().any(|p| p.sha256 == pieces[0].sha256));
}

/// A misheard name counts only at the start, and only as a known mishearing.
#[test]
fn a_misheard_name_counts_only_at_the_start() {
    assert_eq!(atlas::kws::soundalike_at_start("Alice, what time is it?").as_deref(), Some("what time is it?"));
    assert_eq!(atlas::kws::soundalike_at_start("I use Atlassian at work"), None);
    assert!(atlas::kws::knows("Atlas") && !atlas::kws::knows("Jarvis"));
}
