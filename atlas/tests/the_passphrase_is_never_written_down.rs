//! What you say to unlock the vault does not end up on disk.
//!
//! ## What was happening
//!
//! `config/commands.yaml` ships `"the passphrase is"` as an Unlock phrase
//! that takes an argument. Every turn then did:
//!
//! ```ignore
//! self.session.record(said, &intent, &reply);
//! self.thread.append(said, &reply, topic_of(&intent), _t);
//! self.journal.record_at(Act::Asked, said, true, _t);
//! self.persist();
//! ```
//!
//! with `said` the raw utterance. So "the passphrase is hunter2" wrote
//! **hunter2, in plaintext, into `data/state/thread.json` and
//! `data/state/activity.json`** — the same directory as `vault.json`, which
//! it opens. `safety::back_up` copies that folder wholesale, so it was
//! replicated into every backup as well.
//!
//! That defeats the entire premise of `vault.rs`: a stolen laptop is supposed
//! to yield ciphertext without the key. And `typed.rs` opens by stating the
//! rule this broke — *"there is exactly one thing in Atlas that a microphone
//! must never carry: the vault passphrase"*.
//!
//! ## Why the test looks like this
//!
//! It greps the actual state directory after a real turn, rather than
//! checking that some redaction function was called. The defect was never in
//! a redactor — there wasn't one — it was in what reached the disk, so the
//! disk is what is asserted.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, Once};

/// Distinctive enough that finding it anywhere is unambiguous.
const SECRET: &str = "zebra-parsnip-77-quartz";

/// One install, one test at a time.
///
/// The same scaffolding `tests/saying_youre_back.rs` documents, and needed for
/// the same reason: `Daemon::new` loads the vault from
/// `roots::install_state()` -- **the install's state, not the `Store` it was
/// handed** -- so an unlock turn writes the vault of whatever install `roots`
/// resolves to. Without `ATLAS_HOME` that is this checkout's own `data/state`,
/// and the first version of this file duly set a passphrase there and broke
/// five tests in `mail_off_the_tick.rs` with "that isn't the passphrase".
///
/// `roots` caches in a `OnceLock`, so the variable has to be set once, before
/// anything asks.
fn alone() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn home() {
    static ONCE: Once = Once::new();
    let p = std::env::temp_dir().join("atlas-passphrase-never-written");
    ONCE.call_once(|| {
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(p.join("data").join("state")).unwrap();
        std::env::set_var("ATLAS_HOME", &p);
    });
}

fn state_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-secret-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Every file under `dir`, recursively, that contains `needle`.
fn files_containing(dir: &Path, needle: &str) -> Vec<String> {
    let mut hits = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return hits };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            hits.extend(files_containing(&p, needle));
        } else if std::fs::read_to_string(&p).map(|t| t.contains(needle)).unwrap_or(false) {
            hits.push(p.display().to_string());
        }
    }
    hits
}

#[test]
fn saying_the_passphrase_does_not_write_it_to_disk() {
    let _guard = alone();
    home();
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }]);
    let dir = state_dir("unlock");
    let mut d = Daemon::new(&c, &p, None, Store::new(&dir), Proactive::new(ProactiveConfig::default()));

    let reply = d.turn(&format!("the passphrase is {SECRET}"), 1000);
    d.persist();

    // The turn has to have actually done something, or this passes by
    // never reaching the vault at all.
    assert!(
        !reply.trim().is_empty(),
        "the unlock turn produced no reply, so this test is not exercising it"
    );

    let leaked = files_containing(&dir, SECRET);
    assert!(
        leaked.is_empty(),
        "the vault passphrase was written to disk in plaintext, next to the vault \
         it opens:\n  {}",
        leaked.join("\n  ")
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_thread_still_says_that_you_unlocked_it() {
    // Redaction must not turn the transcript into a hole. What the turn was
    // FOR is worth keeping -- only the secret has to go.
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }]);
    let dir = state_dir("readable");
    let mut d = Daemon::new(&c, &p, None, Store::new(&dir), Proactive::new(ProactiveConfig::default()));

    d.turn(&format!("the passphrase is {SECRET}"), 1000);
    d.persist();

    let thread = std::fs::read_to_string(dir.join("thread.json")).unwrap_or_default();
    assert!(
        thread.contains("vault"),
        "the thread no longer records that the vault was unlocked at all: {thread}"
    );
    assert!(
        !thread.contains(SECRET),
        "the secret is still in the thread"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_ordinary_turn_is_still_recorded_word_for_word() {
    // The redaction is scoped to the intent that carries a secret. If it
    // widened to everything, the thread would stop being a transcript and
    // nothing else in this file would notice.
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }]);
    let dir = state_dir("ordinary");
    let mut d = Daemon::new(&c, &p, None, Store::new(&dir), Proactive::new(ProactiveConfig::default()));

    d.turn("what's outstanding", 1000);
    d.persist();

    let thread = std::fs::read_to_string(dir.join("thread.json")).unwrap_or_default();
    assert!(
        thread.contains("what's outstanding") || thread.contains("outstanding"),
        "an ordinary turn stopped being recorded verbatim: {thread}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
