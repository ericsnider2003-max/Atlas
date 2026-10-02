//! The critique on what Atlas writes for you, and "check my writing"
//! (nine-repos report, 1 Oct 2026).

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-writing-checked-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn screen() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

#[test]
fn check_my_writing_quotes_what_it_found_on_the_clipboard() {
    let c = Config::load(Path::new("config")).unwrap();
    let plat = screen();
    let mut d = Daemon::new(&c, &plat, None, Store::new(tmp("slop")), Proactive::new(ProactiveConfig::default()));
    let said = d.turn("check my writing", 1_790_776_800);
    assert!(said.starts_with("Copy the text first"), "{said}");
    plat.set_clipboard("I hope this finds you well. I wanted to reach out about leveraging synergy across the team. I hope this helps!");
    let said = d.turn("check my writing", 1_790_776_830);
    assert!(said.contains("things") || said.starts_with("One thing"), "{said}");
    assert!(said.contains("(\""), "the words that show it are quoted: {said}");
    // Never a rewrite, and never a verdict on who wrote it.
    assert!(!said.to_lowercase().contains("ai wrote"), "{said}");
    assert_eq!(plat.clipboard_now().as_deref().map(|s| s.starts_with("I hope this finds you well")), Some(true));
    plat.set_clipboard("The oven is booked for Tuesday at 6am. Bring the 40 kg flour order and the new scale.");
    assert_eq!(d.turn("is this slop?", 1_790_776_860), "That reads well. Nothing I'd change.");
}

/// A model that writes a stuffy draft first and a clean one when asked to
/// rewrite, so the critique's second call is seen.
struct TwoDrafts(std::sync::Mutex<Vec<String>>);
impl atlas::brain::Llm for TwoDrafts {
    fn complete(&self, system: &str, _user: &str) -> atlas::error::Result<String> {
        self.0.lock().unwrap().push(system.to_string());
        if system.starts_with("You are the person this was written to") {
            Ok("- It doesn't say when the two bakers need to start.".into())
        } else if system.starts_with("Rewrite this to fix the reader's points") {
            Ok("Dear Ms Ortiz,\n\nThe bakery is ready to open on 3 November. We need two more bakers, starting 20 October.\n\nEric".into())
        } else if system == atlas::draft::REVISE_SYSTEM {
            Ok("Dear Ms Ortiz,\n\nThe bakery is ready to open on 3 November. We need two more bakers by then.\n\nEric".into())
        } else {
            Ok("Dear Ms Ortiz,\n\nI hope this finds you well. I wanted to reach out about leveraging synergy with the bakery opening on 3 November.\n\nI hope this helps!\n\nEric".into())
        }
    }
}

#[test]
fn a_written_letter_is_tidied_and_says_what_changed() {
    let mut c = Config::load(Path::new("config")).unwrap();
    let notes = tmp("letter-notes");
    if let Some(t) = c.tools.as_mut() {
        t.research.notes_dir = notes.display().to_string();
    }
    let plat = screen();
    let llm = std::sync::Arc::new(TwoDrafts(Default::default()));
    let mut d = Daemon::new(&c, &plat, Some(llm.clone()), Store::new(tmp("letter")), Proactive::new(ProactiveConfig::default()));
    let mut t = atlas::store::now();
    let said = d.turn("write me a letter to Ms Ortiz about the bakery opening", t);
    assert!(said.starts_with("Writing your letter"), "{said}");
    let mut done = String::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while std::time::Instant::now() < deadline && done.is_empty() {
        t += 1;
        if let Some(s) = d.tick(t).into_iter().find(|s| s.contains("Written:")) {
            done = s;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(done.contains("I tidied"), "the rewrite was made and named: {done}");
    assert!(llm.0.lock().unwrap().iter().any(|s| s == atlas::draft::REVISE_SYSTEM));
    let saved: Vec<_> = std::fs::read_dir(&notes).unwrap().filter_map(|e| e.ok()).filter(|e| e.path().extension().is_some_and(|x| x == "md")).collect();
    let text = std::fs::read_to_string(saved[0].path()).unwrap();
    assert!(text.contains("starting 20 October") && !text.contains("I hope this helps"), "{text}");
    assert!(done.contains("as the person receiving it"), "the blind review is named: {done}");
}
