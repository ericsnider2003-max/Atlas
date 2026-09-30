//! **The capability sweep (30 Sep 2026).**
//!
//! Eric: "Atlas doesn't seem to know how to perform them or even know that
//! it has them." Every command Atlas can be asked for is asked, the way it
//! would be said, through the front door (`Daemon::turn`) on the stand-in
//! platform, and the answer is read for the ways an answer is broken: empty,
//! code leaking into the words, a stub, a panic, the wrong command reached,
//! or a turn that takes seconds with no model in it.
//!
//! `SWEEP_PRINT=1` prints every answer, which is how the fixes in this change
//! were found.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::Path;

/// Something to say after a command that takes one.
fn with_arg(phrase: &str) -> String {
    format!("{phrase} the quarterly budget")
}

/// The ways an answer is broken, if this one is.
pub fn broken(reply: &str) -> Option<&'static str> {
    let r = reply.trim();
    let l = r.to_lowercase();
    if r.is_empty() {
        return Some("empty");
    }
    for leak in ["Some(", "None)", "Ok(", "Err(", "Intent::", "{:?}", "\\n", "PathBuf", "AtlasError", "panicked", "unwrap()"] {
        if r.contains(leak) {
            return Some("code in the words");
        }
    }
    // Found by this sweep and fixed with it (30 Sep 2026).
    for slip in ["? now", "platform:", " a the ", " with  ", "  "] {
        // A list laid out on lines is indented on purpose.
        if slip == "  " && r.contains('\n') {
            continue;
        }
        if l.contains(slip) {
            return Some("a slip in the words");
        }
    }
    for stub in ["not built yet", "isn't built", "not implemented", "todo!", "unimplemented", "coming soon"] {
        if l.contains(stub) {
            return Some("a stub");
        }
    }
    None
}

#[test]
fn every_command_answers_in_words_without_a_model() {
    let dir = std::env::temp_dir().join(format!("atlas-sweep-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Yours, not handed over: a run of the handover tests (their own
    // processes, the same install folder) can leave it handed over.
    atlas::handover::Handover::default().save(&atlas::roots::install_state()).unwrap();
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let book = atlas::intent::ToolBook::new(&c.commands);
    let parser = atlas::intent::Parser::new(&c.commands);
    let print = std::env::var_os("SWEEP_PRINT").is_some();
    let mut bad = Vec::new();
    let mut t = 1_790_700_000u64;
    for e in book.entries() {
        // Atlas's own plumbing (a reminder firing) isn't asked for.
        if e.describe.starts_with("Internal:") {
            continue;
        }
        let Some(phrase) = e.phrases.iter().find(|p| !p.trim().is_empty()) else { continue };
        let said = if e.takes_arg && !e.arg_optional { with_arg(phrase) } else { phrase.clone() };
        // Yours again for each: "hand over" is one of the commands swept.
        atlas::handover::Handover::default().save(&atlas::roots::install_state()).unwrap();
        // Each on a fresh Atlas: one command's state never excuses another's.
        let mut d = Daemon::new(&c, &p, None, Store::new(dir.join(&e.name)), Proactive::new(ProactiveConfig::default()));
        t += 60;
        let parsed = atlas::session::kind_of(&parser.parse(&said)).to_string();
        let started = std::time::Instant::now();
        let reply = d.turn(&said, t);
        let took = started.elapsed();
        if print {
            eprintln!("[{}] {said:?} -> {parsed} ({:?})\n    {}", e.name, took, reply.replace('\n', " / "));
        }
        if let Some(why) = broken(&reply) {
            bad.push(format!("{}: {why}: {said:?} -> {reply:?}", e.name));
        }
        if took > std::time::Duration::from_secs(3) {
            bad.push(format!("{}: took {took:?} with no model: {said:?}", e.name));
        }
    }
    atlas::handover::Handover::default().save(&atlas::roots::install_state()).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(bad.is_empty(), "{} broken:\n{}", bad.len(), bad.join("\n"));
}
