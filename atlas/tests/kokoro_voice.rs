//! Kokoro, spoken inside Atlas (28 Sep 2026).
//!
//! What these hold: the next sentence is made while this one plays and no
//! further; a Kokoro that isn't here falls back to the usual voice, saying so
//! once; the download is pinned like every other; the engine can be chosen in
//! Settings and on the Sound page; and — with the real model, `--ignored` —
//! it actually speaks, measured.

use atlas::kokoro::{self, Ahead, Synth};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A synth that takes `ms` per sentence and notes when each one started and
/// finished, against `t0`.
fn slow_synth(ms: u64, log: Arc<Mutex<Vec<(String, u128, u128)>>>, t0: Instant) -> Synth {
    Arc::new(move |text: &str| {
        let a = t0.elapsed().as_millis();
        std::thread::sleep(Duration::from_millis(ms));
        log.lock().unwrap().push((text.to_string(), a, t0.elapsed().as_millis()));
        Ok(text.as_bytes().to_vec())
    })
}

#[test]
fn the_next_sentence_is_made_while_this_one_plays_and_no_further() {
    let t0 = Instant::now();
    let log = Arc::new(Mutex::new(Vec::new()));
    let ahead = Ahead::default();
    let s: Vec<String> = ["One.", "Two.", "Three."].iter().map(|x| x.to_string()).collect();
    ahead.prepare(s.clone(), slow_synth(150, log.clone(), t0));

    let mut waits = Vec::new();
    let mut taken_at = Vec::new();
    for x in &s {
        let w = Instant::now();
        let got = ahead.take(x).expect("queued").expect("made");
        waits.push(w.elapsed().as_millis());
        taken_at.push(t0.elapsed().as_millis());
        assert_eq!(got, x.as_bytes(), "a sentence came back as another's audio");
        // "Playing" it: longer than it takes to make the next.
        std::thread::sleep(Duration::from_millis(300));
    }
    // The first is waited for; the rest were ready when their turn came.
    assert!(waits[0] >= 100, "the first sentence can't be ready before it's made: {waits:?}");
    assert!(waits[1] < 60 && waits[2] < 60, "the next sentence wasn't made during this one: {waits:?}");
    // One ahead, not all of them at once: "Three." isn't started until "Two."
    // has been taken to play.
    let log = log.lock().unwrap();
    let three = log.iter().find(|(t, _, _)| t == "Three.").expect("Three. was made");
    assert!(three.1 + 5 >= taken_at[1], "Three. started at {} before Two. was taken at {}", three.1, taken_at[1]);
}

#[test]
fn a_sentence_that_isnt_queued_is_left_to_the_caller() {
    let ahead = Ahead::default();
    let t0 = Instant::now();
    ahead.prepare(vec!["Queued.".into()], slow_synth(10, Arc::default(), t0));
    assert!(ahead.take("Something else.").is_none());
    assert!(ahead.has("Queued."), "asking about another sentence dropped the queue");
    assert_eq!(ahead.take("Queued.").unwrap().unwrap(), b"Queued.");
}

#[test]
fn a_new_reply_replaces_what_was_left_of_the_last() {
    let ahead = Ahead::default();
    let t0 = Instant::now();
    ahead.prepare(vec!["A.".into(), "B.".into()], slow_synth(10, Arc::default(), t0));
    assert!(ahead.take("A.").is_some());
    // Interrupted after A.; the next reply starts.
    ahead.prepare(vec!["C.".into()], slow_synth(10, Arc::default(), t0));
    assert!(ahead.take("B.").is_none(), "the old reply's sentence survived a new reply");
    assert_eq!(ahead.take("C.").unwrap().unwrap(), b"C.");
}

#[test]
fn a_skipped_sentence_doesnt_hold_up_the_one_after_it() {
    let ahead = Ahead::default();
    let t0 = Instant::now();
    ahead.prepare(vec!["A.".into(), "B.".into(), "C.".into()], slow_synth(20, Arc::default(), t0));
    // B. asked for first: A. is dropped, B. is made and handed over.
    assert_eq!(ahead.take("B.").unwrap().unwrap(), b"B.");
    assert_eq!(ahead.take("C.").unwrap().unwrap(), b"C.");
    assert!(!ahead.has("A."));
}

#[test]
fn every_voice_on_the_shortlist_is_one_kokoro_has() {
    for (id, _) in atlas::tts::SHORTLIST {
        assert!(kokoro::speaker_id(id).is_some(), "{id} is on the shortlist and not in Kokoro v1.0");
    }
    assert_eq!(kokoro::speaker_id("af_heart"), Some(3));
    assert_eq!(kokoro::speaker_id("em_santa"), Some(53));
    assert_eq!(kokoro::VOICES.len(), 54);
    // A piper voice left in the settings from before the switch.
    assert_eq!(kokoro::voice_or_default("en_US-amy-medium"), ("af_heart", 3));
    assert_eq!(kokoro::voice_or_default("BM_Fable"), ("bm_fable", 25));
    assert!(kokoro::english_voices().iter().all(|v| v.starts_with('a') || v.starts_with('b')));
    assert_eq!(kokoro::display_name("af_heart"), "Heart");
    assert_eq!(kokoro::accent("bm_fable"), "British");
}

#[test]
fn the_download_is_pinned_and_lands_where_kokoro_looks() {
    let m = kokoro::model_piece();
    assert_eq!(m.sha256.len(), 64);
    assert_eq!(m.bytes, 132_303_094);
    assert!(m.url.ends_with("kokoro-int8-multi-lang-v1_0.tar.bz2"));
    assert!(m.key_path().starts_with(kokoro::MODEL_DIR));
    if let Some(r) = kokoro::runtime_piece() {
        assert_eq!(r.sha256.len(), 64);
        assert!(r.url.contains(&format!("/v{}/", kokoro::SHERPA_VERSION)), "the library isn't the version the code speaks to: {}", r.url);
        assert!(r.key_path().starts_with(kokoro::RUNTIME_DIR));
        let (_, capi) = kokoro::runtime_files().unwrap();
        assert!(r.key_path().ends_with(capi));
        assert_eq!(kokoro::pieces().len(), 2);
        let (_, set) = atlas::getpieces::set(Some("kokoro")).expect("`atlas get kokoro`");
        assert_eq!(set, kokoro::pieces());
    }
    assert!(kokoro::download_mb() >= 132);
}

#[test]
fn nothing_downloaded_is_found_missing_in_words() {
    let root = std::env::temp_dir().join(format!("atlas-kokoro-none-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&root);
    if kokoro::runtime_files().is_some() {
        assert_eq!(kokoro::check(&root), Err(kokoro::Missing::Runtime));
        let why = kokoro::engine(&root).err().expect("nothing there, and it loaded");
        assert!(why.contains("isn't downloaded yet") && why.contains("Sound & voice"), "{why}");
        // Library there, model not.
        let rt = root.join(kokoro::RUNTIME_DIR);
        std::fs::create_dir_all(&rt).unwrap();
        let (a, b) = kokoro::runtime_files().unwrap();
        std::fs::write(rt.join(a), b"").unwrap();
        std::fs::write(rt.join(b), b"").unwrap();
        assert_eq!(kokoro::check(&root), Err(kokoro::Missing::Model));
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn falling_back_is_said_once_not_every_sentence() {
    let why = format!("a reason only this test gives {}", std::process::id());
    let first = kokoro::note_once(&why).expect("the first time is said");
    assert!(first.starts_with("Speaking in the usual voice"), "{first}");
    assert!(kokoro::note_once(&why).is_none(), "the same reason was said twice");
}

/// Kokoro chosen and not downloaded: Atlas still speaks, through the
/// configured command, with a piper voice and piper's speed rather than the
/// Kokoro voice's name (which piper would refuse, and Atlas fall silent).
#[cfg(unix)]
#[test]
fn kokoro_chosen_and_missing_still_speaks_in_the_usual_voice() {
    use atlas::tools::ExternalTool;
    if kokoro::check(&atlas::roots::install_root()).is_ok() {
        return; // Kokoro really is installed where this test runs.
    }
    let dir = std::env::temp_dir().join(format!("atlas-kokoro-fallback-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // A "piper" (a shell, by that name) that writes what it was asked for:
    // its voice file and speed, then the words.
    let piper = dir.join("piper");
    let _ = std::fs::remove_file(&piper);
    std::os::unix::fs::symlink("/bin/sh", &piper).unwrap();
    let run = |line: &str, stdin: bool| ExternalTool {
        command: piper.display().to_string(),
        args: vec!["-c".into(), line.into()],
        stdin_text: stdin,
        result_file: None,
        timeout_secs: 30,
    };
    let mut cfg = atlas::voice::ToolsConfig {
        enabled: true,
        work_dir: dir.display().to_string(),
        tts: run("printf '%s %s ' '{voice_file}' '{speed}' > '{out_wav}'; cat >> '{out_wav}'", true),
        play: run("true", false),
        ..Default::default()
    };
    cfg.tts_engine.engine = atlas::tts::Engine::Kokoro;
    cfg.voice_settings.voice = "af_heart".into();
    cfg.voice_settings.speed = 1.25;

    atlas::voice::Voice::new(&cfg).speak("workspace online").unwrap();
    let said = std::fs::read_to_string(dir.join("turn_out.wav")).unwrap();
    assert!(said.ends_with("workspace online"), "nothing was spoken: {said}");
    assert!(said.contains("en_US-amy-medium.onnx"), "piper was handed the Kokoro voice: {said}");
    assert!(said.contains(" 1.25 "), "piper's length scale should be the pace as set: {said}");
    // That it was said once is `falling_back_is_said_once_not_every_sentence`
    // (the note is process-wide, so asserting its text here would race that
    // test).
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_engine_can_be_chosen_in_settings_and_needs_a_restart() {
    let t = atlas::voice::ToolsConfig::default();
    let s = atlas::settings::registry(&t);
    let e = s.get("tts_engine.engine").expect("no engine setting");
    assert!(e.value.as_display().contains("piper"), "{:?}", e.value);
    let mut s2 = s.clone();
    assert!(s2.set("tts_engine.engine", "kokoro").is_ok());
    assert!(s2.set("tts_engine.engine", "espeak").is_err());
    assert!(atlas::settings::needs_a_restart("tts_engine.engine"));

    // Under Kokoro the voice list is Kokoro's.
    let mut k = atlas::voice::ToolsConfig::default();
    k.tts_engine.engine = atlas::tts::Engine::Kokoro;
    let mut ks = atlas::settings::registry(&k);
    assert!(ks.set("voice_settings.voice", "af_heart").is_ok());
    assert!(ks.set("voice_settings.voice", "en_US-amy-medium").is_err());
}

#[test]
fn the_sound_page_offers_the_engine_and_the_download() {
    let view = |engine: atlas::hubpages::EngineView| atlas::hubpages::SoundView {
        engine,
        voices: vec![],
        speed: 1.0,
        speak_replies: "hands_free".into(),
        volume: 100,
        muted: false,
        wake_on: false,
        wake_phrase: "Atlas".into(),
        ptt_on: false,
        ptt_key: "F9".into(),
        typing_key: "Ctrl+Space".into(),
        quiet_on: false,
        quiet_from: "22:00".into(),
        quiet_to: "07:00".into(),
        mics: vec![],
    };
    let missing = atlas::hubpages::sound_page(
        &view(atlas::hubpages::EngineView { kokoro_chosen: true, kokoro_here: true, kokoro_mb: 141, ..Default::default() }),
        None,
    );
    assert!(missing.contains("name=key value=engine") && missing.contains("value=kokoro checked"), "no engine choice");
    assert!(missing.contains("value=get-kokoro") && missing.contains("Get Kokoro · 141 MB"), "no download button");
    assert!(missing.contains("Until Kokoro is here, Atlas speaks in piper"));
    let going = atlas::hubpages::sound_page(
        &view(atlas::hubpages::EngineView {
            kokoro_chosen: true,
            kokoro_here: true,
            kokoro_mb: 141,
            getting: Some("Getting it: 34%".into()),
            ..Default::default()
        }),
        None,
    );
    assert!(going.contains("Getting it: 34%") && !going.contains("value=get-kokoro"), "a second download offered while one runs");
    let ready = atlas::hubpages::sound_page(
        &view(atlas::hubpages::EngineView { kokoro_chosen: true, kokoro_here: true, kokoro_ready: true, kokoro_mb: 141, ..Default::default() }),
        None,
    );
    assert!(!ready.contains("value=get-kokoro") && ready.contains("Kokoro is downloaded"));
}

// ---------------------------------------------------------------- the real thing

/// The real library and model. Set KOKORO_RUNTIME to the folder holding
/// sherpa-onnx's shared library and ONNX Runtime, KOKORO_MODEL to the
/// unpacked kokoro-int8-multi-lang-v1_0, and run with `--ignored
/// --nocapture`. Prints the measurements.
#[test]
#[ignore = "needs the Kokoro library and model (see the comment)"]
fn kokoro_really_speaks_and_is_measured() {
    let rt = std::path::PathBuf::from(std::env::var("KOKORO_RUNTIME").expect("KOKORO_RUNTIME"));
    let model = std::path::PathBuf::from(std::env::var("KOKORO_MODEL").expect("KOKORO_MODEL"));
    let threads: i32 = std::env::var("KOKORO_THREADS").ok().and_then(|t| t.parse().ok()).unwrap_or(kokoro::thread_count());

    let t = Instant::now();
    let k = kokoro::Kokoro::load(&rt, &model, threads).expect("load");
    let load_ms = t.elapsed().as_millis();
    let rate = k.sample_rate();
    assert_eq!(rate, 24_000);
    let sid = kokoro::speaker_id("af_heart").unwrap();

    let short = "Your nine o'clock post is over length.";
    let t = Instant::now();
    let first = k.synth(short, sid, 1.0).unwrap();
    let first_ms = t.elapsed().as_millis();
    let t = Instant::now();
    let again = k.synth(short, sid, 1.0).unwrap();
    let warm_ms = t.elapsed().as_millis();
    let secs = again.len() as f64 / rate as f64;
    assert!(secs > 1.0 && secs < 6.0, "{secs} s for one short sentence");
    let peak = again.iter().fold(0f32, |m, s| m.max(s.abs()));
    assert!(peak > 0.05, "silence (peak {peak})");
    let _ = first;

    let long = "Atlas speaks each reply a sentence at a time. The first one is made while you wait, \
                and every one after it is made while the one before it is playing, so there is no \
                gap between them. This is Kokoro, running on the processor alone.";
    let t = Instant::now();
    let l = k.synth(long, sid, 1.0).unwrap();
    let long_ms = t.elapsed().as_millis() as f64;
    let long_secs = l.len() as f64 / rate as f64;
    let rtf = (long_ms / 1000.0) / long_secs;

    // Pace: slower is longer.
    let slow = k.synth(short, sid, atlas::tts::Engine::Kokoro.speed_value(1.25)).unwrap();
    assert!(slow.len() > again.len(), "a slower pace didn't make it longer");

    // A file a person can listen to.
    if let Ok(out) = std::env::var("KOKORO_OUT") {
        std::fs::write(&out, kokoro::to_wav(&l, rate)).unwrap();
        let wav = std::fs::read(&out).unwrap();
        assert!(atlas::speaking::levels_of_wav(&wav, 30).is_some_and(|v| !v.is_empty()));
    }

    // Sentence by sentence, as a reply is spoken: playback simulated as the
    // audio's own length; the wait before each sentence is measured.
    let k = Arc::new(Mutex::new(k));
    let kk = k.clone();
    let synth: Synth = Arc::new(move |text: &str| {
        let k = kk.lock().unwrap();
        k.synth(text, sid, 1.0).map(|s| kokoro::to_wav(&s, rate))
    });
    let sentences: Vec<String> = atlas::speech::split(long);
    let ahead = Ahead::default();
    let wall = Instant::now();
    ahead.prepare(sentences.clone(), synth);
    let mut waits = Vec::new();
    for s in &sentences {
        let w = Instant::now();
        let wav = ahead.take(s).unwrap().unwrap();
        waits.push(w.elapsed().as_millis());
        let play = (wav.len() - 44) as f64 / 2.0 / rate as f64;
        std::thread::sleep(Duration::from_secs_f64(play));
    }
    let wall_ms = wall.elapsed().as_millis() as f64;
    // One after another -- make, play, make, play -- would take the whole
    // synthesis plus the whole audio.
    let one_after_another = long_ms + long_secs * 1000.0;
    let load = std::fs::read_to_string("/proc/loadavg").unwrap_or_default();

    println!(
        "KOKORO threads={threads} load={load_ms}ms first_short={first_ms}ms warm_short={warm_ms}ms \
         short_audio={secs:.2}s long_audio={long_secs:.2}s long_synth={long_ms:.0}ms RTF={rtf:.3} \
         sentences={} waits_before_each_ms={waits:?} spoken_in={wall_ms:.0}ms one_after_another={one_after_another:.0}ms \
         loadavg={}",
        sentences.len(),
        load.trim()
    );
    assert!(
        wall_ms < one_after_another * 0.95,
        "making the next sentence while this one plays saved nothing: {wall_ms:.0} ms vs {one_after_another:.0} ms"
    );
    // Gapless is only possible when a sentence is made faster than it is said.
    if rtf < 0.8 {
        assert!(waits.iter().skip(1).all(|w| *w < 150), "a gap between sentences on a machine fast enough for none: {waits:?}");
    }
}

/// Atlas's own speaking path, end to end, in Kokoro: a reply handed over
/// whole (`Voice::prepare`, as `Daemon::say_interruptibly` does), then each
/// sentence spoken (`Voice::speak`), each played by the configured player.
/// Needs Kokoro downloaded into ATLAS_HOME (`atlas get kokoro`); run with
/// `ATLAS_HOME=<that folder> ... -- --ignored`.
#[cfg(unix)]
#[test]
#[ignore = "needs Kokoro downloaded into ATLAS_HOME"]
fn a_reply_is_spoken_in_kokoro_sentence_by_sentence() {
    use atlas::daemon::Mouth;
    use atlas::tools::ExternalTool;
    let root = atlas::roots::install_root();
    kokoro::check(&root).expect("Kokoro isn't in ATLAS_HOME");
    let dir = std::env::temp_dir().join(format!("atlas-kokoro-reply-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // The player keeps a copy of each thing it plays.
    let play = ExternalTool {
        command: "sh".into(),
        args: vec!["-c".into(), format!("cp '{{out_wav}}' '{}'/played-$(date +%s%N).wav", dir.display())],
        stdin_text: false,
        result_file: None,
        timeout_secs: 30,
    };
    let mut cfg = atlas::voice::ToolsConfig {
        enabled: true,
        work_dir: dir.join("work").display().to_string(),
        play,
        ..Default::default()
    };
    cfg.tts_engine.engine = atlas::tts::Engine::Kokoro;
    cfg.voice_settings.voice = "bm_fable".into();
    let voice = atlas::voice::Voice::new(&cfg);
    let reply = "Your post is ready. It's twelve characters over. Shall I trim it?";
    let t = Instant::now();
    voice.prepare(reply);
    let mut per = Vec::new();
    for chunk in atlas::speech::split(reply) {
        let s = Instant::now();
        voice.speak(&atlas::spoken_form::for_speech(&chunk)).expect("spoke");
        per.push((s.elapsed().as_millis(), voice.last_speak_split_ms()));
    }
    let mut played: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "wav")).collect();
    played.sort();
    println!("KOKORO-REPLY total={}ms per_sentence(ms, (synth_wait, play))={per:?}", t.elapsed().as_millis());
    assert_eq!(played.len(), 3, "three sentences, three things played: {played:?}");
    for p in &played {
        let wav = std::fs::read(p).unwrap();
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 24_000, "not Kokoro's audio: {}", p.display());
        let levels = atlas::speaking::levels_of_wav(&wav, 30).unwrap();
        assert!(levels.len() > 20 && levels.iter().any(|l| *l > 200), "silent: {}", p.display());
    }
    let _ = std::fs::remove_dir_all(&dir);
}
