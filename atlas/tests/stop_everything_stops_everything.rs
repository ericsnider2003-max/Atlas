//! The phrase Atlas teaches you for when it has got something wrong.
//!
//! `firstrun.rs` ends setup with: *"Say \"what can you do\" any time,
//! \"stop everything\" if I get something wrong."*
//!
//! It did nothing. `attention::hear` recognised the phrase and returned
//! `Heard::Panic`; the daemon's match on that result had no arm for it, so it
//! fell through to ordinary parsing and came back as "I don't know that".
//! `Attention::halt` — which empties the queues, abandons the work and sets
//! `halted` so a later resume does not silently restart it — had **no caller
//! in `src/` at all**. Built, tested, taught to the user by name, wired to
//! nothing.
//!
//! A check that runs and whose answer is thrown away is worse than no check,
//! because it reads as covered.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-panic-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}


#[test]
fn the_phrase_firstrun_teaches_is_recognised() {
    // Whatever `firstrun` promises must be a phrase `hear` knows. Read out of
    // the source rather than hard-coded, so changing the promise without
    // changing the handler fails here.
    let fr = std::fs::read_to_string("src/firstrun.rs").expect("firstrun.rs");
    let promised: Vec<&str> = fr
        .match_indices("stop everything")
        .map(|_| "stop everything")
        .collect();
    assert!(
        !promised.is_empty(),
        "firstrun no longer teaches an emergency-stop phrase -- if it moved, point \
         this test at the new one rather than deleting it"
    );
    for phrase in promised {
        assert_eq!(
            atlas::attention::hear(phrase),
            Some(atlas::attention::Heard::Panic),
            "{phrase:?} is taught to the user and not recognised"
        );
    }
}

#[test]
fn saying_it_actually_halts() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let dir = tmp("halt");
    let mut d = Daemon::new(&c, &p, None, Store::new(&dir), Proactive::new(ProactiveConfig::default()));

    let reply = d.turn("stop everything", 1000);

    assert!(
        !reply.to_lowercase().contains("don't know"),
        "the emergency stop came back as an unknown command: {reply:?}"
    );
    assert!(
        d.attention.was_halted(),
        "nothing was halted -- `Attention::halt` was not reached. Reply was {reply:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn every_panic_phrase_halts_not_just_the_taught_one() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    for (i, phrase) in ["stop everything", "emergency stop", "halt", "drop everything"]
        .iter()
        .enumerate()
    {
        if atlas::attention::hear(phrase) != Some(atlas::attention::Heard::Panic) {
            continue; // not a panic phrase in this build; the one above covers the promise
        }
        let dir = tmp(&format!("p{i}"));
        let mut d = Daemon::new(&c, &p, None, Store::new(&dir), Proactive::new(ProactiveConfig::default()));
        d.turn(phrase, 1000);
        assert!(d.attention.was_halted(), "{phrase:?} was recognised as a panic and did not halt");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn it_can_interrupt_atlas_mid_sentence() {
    // The commonest moment to need an emergency stop is while Atlas is
    // talking. `speech::is_interruption` decides what may barge in, and it
    // listed Pause and Cancel but not Panic -- so the stop phrase could not
    // interrupt the thing you wanted stopped.
    assert!(
        atlas::speech::is_interruption("stop everything"),
        "the emergency stop cannot interrupt Atlas while it is speaking"
    );
}
