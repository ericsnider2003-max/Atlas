//! Phase 0.2 and 0.8 (Eric's go-ahead, 1 Oct 2026): measure the silence
//! Eric actually sits through, stop the half-hour model restarts, and make
//! the voice keep up.
//!
//! From the laptop's records (30 Sep): "doing" included the reply's playback;
//! about 60 of 107 turns carried listening figures from an earlier turn; the
//! model server was stopped "idle" and restarted within seconds 13 times with
//! nobody talking; Kokoro took about 1.3x as long to make a sentence as to
//! play it.

use atlas::lifecycle::{model_stays_when_it_fits, Helpers, LifecycleConfig, MODEL_RESIDENT_SECS};

#[test]
fn the_silence_is_hearing_plus_the_wait_for_the_first_sound() {
    use atlas::timing::silence_before_first_sound as silence;
    assert_eq!(silence(Some(800), Some(1700)), Some(2500));
    // A typed turn has no end of speech; a reply that never played has no
    // first sound. Missing, never guessed as zero.
    assert_eq!(silence(None, Some(1700)), None);
    assert_eq!(silence(Some(800), None), None);
}

#[test]
fn the_first_sound_is_the_first_playback_after_the_turn_began() {
    use atlas::speaking::{heard_now, listen_for_first_sound, take_first_sound};
    // Nothing armed: a line said between turns is not a turn's first sound.
    let _ = take_first_sound();
    heard_now();
    assert!(take_first_sound().is_none());

    listen_for_first_sound();
    let before = std::time::Instant::now();
    heard_now();
    std::thread::sleep(std::time::Duration::from_millis(5));
    heard_now(); // the second sentence doesn't move it
    let first = take_first_sound().expect("the first sound was kept");
    assert!(first >= before && first.elapsed() >= std::time::Duration::from_millis(5));
    // Taken once.
    assert!(take_first_sound().is_none());
}

#[test]
fn a_machine_with_room_keeps_the_model_loaded() {
    let cfg = model_stays_when_it_fits(LifecycleConfig::default(), true);
    assert_eq!(cfg.keep_warm_secs.get("model-server"), Some(&MODEL_RESIDENT_SECS));
    let mut h = Helpers::new(cfg);
    h.want("model-server", 0, 1, || Ok(None)).unwrap();
    h.done("model-server", 1);
    // An hour and a half with nobody talking: on 30 Sep this was three stops
    // and three cold starts.
    let _ = h.reap(1 + 5400);
    assert!(h.is_running("model-server"));
}

#[test]
fn a_machine_without_room_still_gives_the_memory_back() {
    let cfg = model_stays_when_it_fits(LifecycleConfig::default(), false);
    assert_eq!(cfg.keep_warm_secs.get("model-server"), Some(&1800));
    let mut h = Helpers::new(cfg);
    h.want("model-server", 0, 1, || Ok(None)).unwrap();
    h.done("model-server", 1);
    let _ = h.reap(1 + 1801);
    assert!(!h.is_running("model-server"));
}

#[test]
fn a_longer_setting_of_your_own_is_left_alone() {
    let mut cfg = LifecycleConfig::default();
    cfg.keep_warm_secs.insert("model-server".into(), MODEL_RESIDENT_SECS * 2);
    let cfg = model_stays_when_it_fits(cfg, true);
    assert_eq!(cfg.keep_warm_secs.get("model-server"), Some(&(MODEL_RESIDENT_SECS * 2)));
}

#[test]
fn the_model_server_is_lowered_only_while_a_reply_plays() {
    use atlas::voicefirst::{model_lowered, model_started, speaking};
    model_started(0); // no real process: only the state is switched
    assert!(!model_lowered());
    speaking(true);
    assert!(model_lowered());
    speaking(true); // a second sentence changes nothing
    assert!(model_lowered());
    speaking(false);
    assert!(!model_lowered());
}

#[test]
fn the_lines_atlas_says_most_are_made_ahead() {
    // Ready before they're needed; a change of voice makes them stale.
    for line in ["Yes?", "Looking now.", "Sorry about that."] {
        assert!(atlas::kokoro::STOCK_LINES.contains(&line), "{line}");
    }
    assert!(atlas::kokoro::STOCK_LINES.contains(&atlas::daemon::STILL_LOADING_WORDS));
    let made = std::sync::Arc::new(|t: &str| Ok::<Vec<u8>, String>(t.as_bytes().to_vec()));
    let with = atlas::kokoro::made_for("am_puck", 0.85, 100);
    atlas::kokoro::prepare_stock(made, with.clone());
    let mut got = None;
    for _ in 0..200 {
        got = atlas::kokoro::stock("Yes?", &with);
        if got.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(got.as_deref(), Some(&b"Yes?"[..]));
    // Not a stock line, or made for another voice: made fresh, as before.
    assert!(atlas::kokoro::stock("What's the weather?", &with).is_none());
    assert!(atlas::kokoro::stock("Yes?", &atlas::kokoro::made_for("af_heart", 1.0, 100)).is_none());
}

#[test]
fn friends_get_kokoro_first_with_piper_behind_it() {
    let shipped = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/config/tools.yaml")).unwrap();
    let v: serde_yaml::Value = serde_yaml::from_str(&shipped).unwrap();
    assert_eq!(v["tts_engine"]["engine"].as_str(), Some("kokoro"));
    // The fallback is still piper's command, and setup fetches Kokoro itself.
    assert!(v["tts"]["command"].as_str().unwrap_or("").contains("piper"));
    assert!(atlas::getpieces::setup_pieces().iter().any(|p| format!("{p:?}").to_lowercase().contains("kokoro")));
}
