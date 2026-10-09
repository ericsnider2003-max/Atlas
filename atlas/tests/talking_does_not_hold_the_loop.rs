//! **The talk key records on the microphone's thread (30 Sep 2026).**
//!
//! Holding the talk key used to record, and transcribe, on the main loop:
//! for as long as you talked and then while the words were made out, the hub
//! didn't answer, the typing box didn't open and nothing else moved. Now the
//! microphone's own thread records while the key is held and hands the words
//! back, and the loop only takes them.

use atlas::micthread::{Heard, MicStream, MicThread, MicWork};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct KeyMic {
    wakes: Arc<AtomicUsize>,
    recording: Arc<AtomicBool>,
}

impl MicWork for KeyMic {
    fn wake_once(&mut self, stop: &dyn Fn() -> bool) -> atlas::error::Result<bool> {
        self.wakes.fetch_add(1, Ordering::SeqCst);
        while !stop() {
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(false)
    }
    fn listen(&mut self) -> atlas::error::Result<String> {
        Ok(String::new())
    }
    fn listen_while(&mut self, held: &dyn Fn() -> bool) -> atlas::error::Result<Option<String>> {
        self.recording.store(true, Ordering::SeqCst);
        while held() {
            std::thread::sleep(Duration::from_millis(5));
        }
        // Making out the words takes a while too.
        std::thread::sleep(Duration::from_millis(150));
        self.recording.store(false, Ordering::SeqCst);
        Ok(Some("what's on my calendar tomorrow".into()))
    }
    fn open_stream(&mut self) -> Option<Box<dyn MicStream>> {
        None
    }
    fn transcribe(&mut self, _samples: &[i16]) -> atlas::error::Result<String> {
        Ok(String::new())
    }
    fn follow_up(&mut self, _secs: u32, _stop: &dyn Fn() -> bool) -> atlas::error::Result<Option<String>> {
        Ok(None)
    }
    fn models_dir(&self) -> PathBuf {
        PathBuf::from("tests/fixtures/silero")
    }
}

fn wait_until(what: &str, secs: u64, mut f: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < until {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("waited {secs}s for {what}");
}

#[test]
fn the_words_come_back_while_the_loop_carries_on() {
    let wakes = Arc::new(AtomicUsize::new(0));
    let recording = Arc::new(AtomicBool::new(false));
    let mic = MicThread::start(Box::new(KeyMic { wakes: wakes.clone(), recording: recording.clone() }));
    mic.set_wake(true);
    wait_until("the wake word to be listened for", 2, || wakes.load(Ordering::SeqCst) >= 1);

    let down = Arc::new(AtomicBool::new(true));
    let d = down.clone();
    let t = Instant::now();
    mic.talk(Arc::new(move || d.load(Ordering::SeqCst)));
    crate::common::assert_prompt(t.elapsed(), Duration::from_millis(20), "asking took");
    wait_until("the key's recording to start", 2, || recording.load(Ordering::SeqCst));

    // While you talk, the loop's look at the microphone takes no time.
    for _ in 0..10 {
        let t = Instant::now();
        assert!(mic.poll().is_none());
        crate::common::assert_prompt(t.elapsed(), Duration::from_millis(10), "took too long");
        std::thread::sleep(Duration::from_millis(10));
    }
    let wakes_while_talking = wakes.load(Ordering::SeqCst);
    down.store(false, Ordering::SeqCst);

    let mut got = None;
    wait_until("the words", 3, || {
        got = mic.poll();
        got.is_some()
    });
    match got {
        Some(Heard::Talk(Ok(Some(said)), held)) => {
            assert_eq!(said, "what's on my calendar tomorrow");
            assert!(held >= 0.09, "held for {held}s");
        }
        other => panic!("expected the words, got {:?}", other.map(|_| "something else")),
    }
    // The wake word gave way while the key was held.
    assert_eq!(wakes.load(Ordering::SeqCst), wakes_while_talking);
}
