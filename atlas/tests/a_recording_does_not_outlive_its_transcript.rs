//! `retention::discard_audio` had no caller, and the cleanup that stood in
//! for it leaked the file on every error path.
//!
//! ## The promise, and what backed it
//!
//! `discard_audio`'s doc:
//!
//! > Called immediately after transcription rather than on a timer, so the
//! > window in which a recording of your voice exists on disk is measured in
//! > seconds rather than hours. The transcript is the thing you wanted; the
//! > audio is a byproduct, and keeping it "just in case" is how it ends up in
//! > a backup somewhere.
//!
//! Nothing in `src/` called it. `delete_audio_after_transcribing` — a setting
//! a person can turn on and off — decided nothing.
//!
//! The two places that extract audio did their own cleanup: a hard-coded
//! `let _ = std::fs::remove_file(&wav)` on the last line of the happy path.
//! So the setting was ignored, the result was discarded, and every early
//! return left the recording behind:
//!
//! * `listen_to` returns early on *"I couldn't get the sound out of that"*
//!   and on *"There was nothing said in that."*
//! * `transcribe_timed` returns early when ffmpeg fails.
//!
//! Those are the common endings for a bad recording, not the rare ones. And
//! the transcriber's own output — whisper's `.srt` and `.txt`, the same words
//! in another form — was written beside the audio and never removed at all.
//!
//! The sentence about ending up in a backup is no longer hypothetical either:
//! `back_up` walks subfolders now, so anything left in a scratch directory
//! under the state folder goes into every backup from here on.
//!
//! ## Why a guard
//!
//! A cleanup that has to be repeated on every `return` is one that gets
//! forgotten on the next `return` somebody adds — which is exactly how this
//! happened. `retention::Recording` removes on drop, so there is no way out
//! of the function that skips it.

use atlas::retention::{discard_audio, Recording, RetentionConfig};
use std::path::PathBuf;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-rec-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn on() -> RetentionConfig {
    RetentionConfig { delete_audio_after_transcribing: true, ..Default::default() }
}

fn off() -> RetentionConfig {
    RetentionConfig { delete_audio_after_transcribing: false, ..Default::default() }
}

#[test]
fn the_shipped_default_removes_the_recording() {
    // The setting this rests on. If it ever ships off, everything below is
    // about a code path nobody is on.
    assert!(
        RetentionConfig::default().delete_audio_after_transcribing,
        "Atlas ships keeping recordings of your voice"
    );
}

#[test]
fn a_recording_is_gone_when_the_work_finishes() {
    let dir = tmp("finishes");
    let wav = dir.join("heard.wav");
    std::fs::write(&wav, b"RIFF....").unwrap();

    {
        let _r = Recording::new(&wav, &on());
        assert!(wav.exists(), "it removed the file before the work was done");
    }
    assert!(!wav.exists(), "the recording outlived the transcription");
}

#[test]
fn a_recording_is_gone_even_when_the_work_fails_part_way() {
    // The whole point. This is the shape of `listen_to`'s two early returns
    // and of `transcribe_timed`'s ffmpeg check — the endings a bad recording
    // actually takes.
    let dir = tmp("early-return");
    let wav = dir.join("heard.wav");
    std::fs::write(&wav, b"RIFF....").unwrap();

    fn transcribe(wav: &std::path::Path, cfg: &RetentionConfig) -> Result<String, String> {
        let _r = Recording::new(wav, cfg);
        // "There was nothing said in that."
        Err("nothing said".into())
    }

    assert!(transcribe(&wav, &on()).is_err());
    assert!(
        !wav.exists(),
        "a transcription that gave up left the recording of your voice on disk"
    );
}

#[test]
fn a_panic_does_not_leave_the_recording_behind_either() {
    // Drop runs while unwinding, which is the difference between a guard and
    // a line at the end of the function. `crash::caught` wraps real work in
    // `catch_unwind`, so this is a path Atlas genuinely takes.
    let dir = tmp("panic");
    let wav = dir.join("heard.wav");
    std::fs::write(&wav, b"RIFF....").unwrap();

    let cfg = on();
    let w = wav.clone();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _r = Recording::new(&w, &cfg);
        panic!("the transcriber fell over");
    }));

    assert!(!wav.exists(), "a panic mid-transcription left the recording on disk");
}

#[test]
fn what_the_transcriber_writes_beside_it_goes_too() {
    // whisper's `.srt` and `.txt` are the same speech in another form, and
    // the old cleanup removed neither.
    let dir = tmp("alongside");
    let wav = dir.join("heard.wav");
    let srt = dir.join("heard.srt");
    let txt = dir.join("heard.txt");
    for p in [&wav, &srt, &txt] {
        std::fs::write(p, b"words").unwrap();
    }

    {
        let mut r = Recording::new(&wav, &on());
        r.and_also(&srt).and_also(&txt);
    }
    assert!(!wav.exists(), "the audio stayed");
    assert!(!srt.exists(), "the subtitles stayed — the same words in another file");
    assert!(!txt.exists(), "the transcript file stayed");
}

#[test]
fn turning_the_setting_off_actually_keeps_it() {
    // The setting decided nothing, in either direction. Someone who turns it
    // off — to keep recordings for a while, deliberately — was getting them
    // deleted anyway by the hard-coded `remove_file`.
    let dir = tmp("keeps");
    let wav = dir.join("heard.wav");
    std::fs::write(&wav, b"RIFF....").unwrap();

    {
        let _r = Recording::new(&wav, &off());
    }
    assert!(wav.exists(), "it deleted a recording the setting said to keep");
}

#[test]
fn discard_audio_itself_still_answers_honestly() {
    let dir = tmp("direct");
    let wav = dir.join("heard.wav");
    std::fs::write(&wav, b"RIFF....").unwrap();

    assert!(!discard_audio(&wav, &off()), "it reported deleting something the setting keeps");
    assert!(wav.exists());
    assert!(discard_audio(&wav, &on()));
    assert!(!wav.exists());
    // A second go has nothing to remove, and says so rather than claiming it
    // did something.
    assert!(!discard_audio(&wav, &on()), "it reported deleting a file that was not there");
}

#[test]
fn the_guard_is_what_the_transcription_paths_actually_use() {
    // Otherwise this file tests a struct nobody constructs — the same shape
    // as the defect it is about. `discard_audio` had a doc, a config flag,
    // tests, and no caller.
    // Each file's own tests cut off on their own: one `#[cfg(test)]` in an
    // earlier file (making.rs, 2 Oct) must not hide every file after it.
    let live: String = crate::common::source_files_of("daemon")
        .iter()
        .map(|f| {
            let text = std::fs::read_to_string(f).unwrap_or_default();
            match text.find("#[cfg(test)]") {
                Some(at) => text[..at].to_string(),
                None => text,
            }
        })
        .collect();
    let uses = live.matches("retention::Recording::new(").count();
    assert!(
        uses >= 2,
        "only {uses} of the two audio paths use the guard; the other is back to a \
         `remove_file` on the happy path, which leaks on every early return"
    );
    assert!(
        !live.contains("let _ = std::fs::remove_file(&wav)"),
        "a transcription path is removing the recording by hand again"
    );
}
