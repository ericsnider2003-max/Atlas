//! The clipboard round trip, wired to a real OS clipboard through the platform.
//!
//! The module always classified what was copied and drafted an answer, but two
//! ends were hollow: nothing read the OS clipboard in production (a test set the
//! field by hand), and nothing wrote the answer back — yet the reply claimed
//! "it's back on your clipboard" every time. These drive the wired ends through
//! `MockPlatform`'s real in-memory clipboard and hold the promise to the truth.

use atlas::brain::{Llm, MockLlm};
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::{Monitor, Platform};
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::Path;
use std::sync::Arc;

fn dirp(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("atlas-clip-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn one_monitor() -> Vec<Monitor> {
    vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]
}

#[test]
fn what_was_copied_is_read_from_the_os_clipboard_when_nothing_set_it() {
    // No test seeds `clipboard_text`; the copied text lives on the platform's
    // clipboard, exactly as it would in production. The daemon must read it.
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(one_monitor());
    p.set_clipboard("thread 'main' panicked at src/lib.rs line twelve");
    let mut d = Daemon::new(&c, &p, None, Store::new(dirp("read")), Proactive::new(ProactiveConfig::default()));

    // No model, so it describes what it picked up rather than answering — but
    // to describe it, it had to have read the OS clipboard. It classified the
    // panic text as an error.
    let reply = d.turn("explain this", 100);
    assert!(reply.to_lowercase().contains("error"), "should have read and classified the copied error: {reply}");
    // Proof it actually read the clipboard rather than emitting a fixed line:
    // a different copied text on a second machine yields a different reply.
    let p2 = MockPlatform::new(one_monitor());
    p2.set_clipboard("just some ordinary notes with nothing wrong in them");
    let mut d2 = Daemon::new(&c, &p2, None, Store::new(dirp("read-alt")), Proactive::new(ProactiveConfig::default()));
    assert_ne!(d2.turn("explain this", 100), reply, "the copied text must drive the description");
}

#[test]
fn with_no_clipboard_tool_it_says_so_rather_than_claiming_empty() {
    // A fresh platform with nothing on its clipboard returns None (no tool /
    // nothing to read). That must read as "I can't reach it", not "it's empty".
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(one_monitor());
    let mut d = Daemon::new(&c, &p, None, Store::new(dirp("none")), Proactive::new(ProactiveConfig::default()));

    let reply = d.turn("explain this", 100);
    assert!(reply.to_lowercase().contains("can't reach") || reply.to_lowercase().contains("no clipboard tool"), "{reply}");
    // And it is the unreachable-clipboard case specifically: with something to
    // read, the answer is different.
    let p2 = MockPlatform::new(one_monitor());
    p2.set_clipboard("a line of text that is definitely present to read");
    let mut d2 = Daemon::new(&c, &p2, None, Store::new(dirp("some")), Proactive::new(ProactiveConfig::default()));
    assert_ne!(d2.turn("explain this", 100), reply, "reaching the clipboard must change the answer");
}

#[test]
fn the_answer_actually_lands_back_on_the_clipboard() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(one_monitor());
    p.set_clipboard("some prose worth summarising, a few words at least here");
    let answer = "A short summary of the prose.";
    let llm: Arc<dyn Llm> = Arc::new(MockLlm(answer.into()));
    let mut d = Daemon::new(&c, &p, Some(llm), Store::new(dirp("write")), Proactive::new(ProactiveConfig::default()));

    let reply = d.turn("summarise this", 100);
    // The claim is only made when it's true — and here it is true: the mock
    // clipboard now holds exactly the answer.
    assert!(reply.contains("back on your clipboard"), "should confirm the write-back: {reply}");
    assert_eq!(p.clipboard_now().as_deref(), Some(answer), "the answer must actually be on the clipboard");
}

/// A platform that can read a clipboard but can't write one — the trait's
/// default `write_clipboard` refuses. Stands in for a machine with no
/// clipboard-write tool installed.
struct ReadOnlyClipboard(std::cell::RefCell<Option<String>>);
impl Platform for ReadOnlyClipboard {
    fn monitors(&self) -> atlas::error::Result<Vec<Monitor>> {
        Ok(one_monitor())
    }
    fn launch(&self, _: &atlas::config::AppSpec) -> atlas::error::Result<()> {
        Ok(())
    }
    fn find_window(&self, _: &atlas::config::AppSpec) -> atlas::error::Result<Option<atlas::platform::WindowId>> {
        Ok(None)
    }
    fn place(&self, _: atlas::platform::WindowId, _: atlas::platform::PixelRect) -> atlas::error::Result<()> {
        Ok(())
    }
    fn focus(&self, _: atlas::platform::WindowId) -> atlas::error::Result<()> {
        Ok(())
    }
    fn close(&self, _: &atlas::config::AppSpec) -> atlas::error::Result<()> {
        Ok(())
    }
    fn sleep_ms(&self, _: u64) {}
    fn read_clipboard(&self) -> atlas::error::Result<Option<String>> {
        Ok(self.0.borrow().clone())
    }
    // write_clipboard falls through to the trait default, which refuses.
}

#[test]
fn when_the_write_back_fails_it_says_so_and_does_not_pretend() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = ReadOnlyClipboard(std::cell::RefCell::new(Some(
        "some prose worth summarising, a few words at least here".into(),
    )));
    let answer = "A short summary of the prose.";
    let llm: Arc<dyn Llm> = Arc::new(MockLlm(answer.into()));
    let mut d = Daemon::new(&c, &p, Some(llm), Store::new(dirp("rofail")), Proactive::new(ProactiveConfig::default()));

    let reply = d.turn("summarise this", 100);
    // The answer is still shown, but the false "it's on your clipboard" claim
    // is gone — it says plainly it couldn't put it there.
    assert!(reply.contains(answer), "the answer is still given: {reply}");
    assert!(reply.to_lowercase().contains("couldn't put it back"), "honest about the failure: {reply}");
    assert!(!reply.contains("back on your clipboard"), "must not claim success it didn't have: {reply}");
    // The honesty note is appended, not swapped in for the answer: the reply
    // carries strictly more than the bare answer.
    assert!(reply.len() > answer.len(), "the couldn't-put-back note is added, not a replacement: {reply}");
}
