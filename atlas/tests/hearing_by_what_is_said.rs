//! The wake word listened for by what's said, not by three-second clips
//! (30 Sep 2026), through the real hearing server and model.
//!
//! Its own test target because it sets `ATLAS_HOME` for the process (the
//! install root is resolved once). Runs when `ATLAS_PARAKEET_KIT` names a
//! folder holding `tools/sherpa`, `models/parakeet`, `fake-recorder.sh` (a
//! "microphone" that plays `stream.raw`: someone else talking, a pause, then
//! "Atlas, can you see me?") -- otherwise it passes without running.

use atlas::micthread::MicWork;

#[test]
fn the_name_is_heard_in_a_stream_and_what_followed_it_is_kept() {
    let Ok(kit) = std::env::var("ATLAS_PARAKEET_KIT") else { return };
    std::env::set_var("ATLAS_HOME", &kit);
    let c = atlas::config::Config::load(std::path::Path::new("config")).unwrap();
    let mut tc = c.tools.clone().unwrap();
    tc.enabled = true;
    tc.record.command = format!("{kit}/fake-recorder.sh");
    tc.vars.insert("mic_device".into(), "fake".into());
    tc.endpoint.enabled = true;
    tc.work_dir = std::env::temp_dir().join(format!("atlas-hearing-stream-{}", std::process::id())).display().to_string();
    let wake = tc.wake.as_mut().expect("the shipped config has a wake section");
    wake.enabled = true;
    wake.phrase = "atlas".into();
    wake.detector = None;
    let voice = atlas::voice::Voice::new(&tc);
    let mut work = voice.mic_work();
    let started = std::time::Instant::now();
    let heard = work.wake_once(&|| false).expect("listened");
    let took = started.elapsed();
    let after = work.take_said_with_wake();
    atlas::parakeet::stop();
    println!("heard the name: {heard}, after it: {after:?}, in {took:?}");
    assert!(heard, "the name was in the stream");
    let after = after.unwrap_or_default().to_lowercase();
    assert!(after.contains("see me"), "what followed the name: {after:?}");
    // The other talk before it was heard and passed over, not taken as a request.
    assert!(!after.contains("painting"));
}
