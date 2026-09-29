//! The timing window is actually filled by a turn.
//!
//! `timing.rs` arrived complete and tested, and nothing anywhere called
//! `Recent::add`. Every one of its 20 tests passed, because they all build a
//! `Recent` by hand. The daemon held a window that stayed empty forever.
//!
//! That is the `hollow` shape exactly: an empty window answers "Nothing's been
//! slow" and reports no `GotSlower` signal, which is indistinguishable from a
//! fast machine. `got_slower()` needs ten turns before it will even look, so
//! nothing could ever have raised it.
//!
//! These tests fail if the wiring is removed. They exercise the two real
//! entry points — the typed prompt (`execute_timed`) and a spoken exchange
//! (`converse`) — rather than constructing a `Recent` and asserting about it.

use atlas::config::Config;
use atlas::daemon::{Daemon, Ears, Mouth};
use atlas::error::Result;
use atlas::intent::Intent;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use atlas::timing::Stage;
use std::cell::Cell;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-timing-wired-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }])
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(
        c,
        p,
        None,
        Store::new(tmp(tag)),
        Proactive::new(ProactiveConfig::default()),
    )
}

// ---------------------------------------------------------------------------
// Test doubles
// ---------------------------------------------------------------------------

/// Ears that report a split, the way the real `Voice` does.
struct SplitEars {
    listening_ms: u32,
    hearing_ms: u32,
    followups: Cell<u32>,
}

impl Ears for SplitEars {
    fn wait_for_wake(&self) -> Result<()> {
        Ok(())
    }
    fn listen(&self) -> Result<String> {
        Ok("hello".into())
    }
    fn listen_briefly(&self, _secs: u32) -> Result<Option<String>> {
        // One follow-up, then silence, so `converse` terminates.
        let n = self.followups.get();
        self.followups.set(n + 1);
        Ok(None)
    }
    fn last_listen_split_ms(&self) -> Option<(u32, u32)> {
        Some((self.listening_ms, self.hearing_ms))
    }
}

/// Ears that cannot tell recording from transcribing — the default.
struct MuteEars;

impl Ears for MuteEars {
    fn wait_for_wake(&self) -> Result<()> {
        Ok(())
    }
    fn listen(&self) -> Result<String> {
        Ok("hello".into())
    }
    fn listen_briefly(&self, _secs: u32) -> Result<Option<String>> {
        Ok(None)
    }
}

struct SplitMouth {
    speaking_ms: u32,
    playing_ms: u32,
}

impl Mouth for SplitMouth {
    fn speak(&self, _text: &str) -> Result<()> {
        Ok(())
    }
    fn last_speak_split_ms(&self) -> Option<(u32, u32)> {
        Some((self.speaking_ms, self.playing_ms))
    }
}

struct MuteMouth;

impl Mouth for MuteMouth {
    fn speak(&self, _text: &str) -> Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------

#[test]
fn a_typed_turn_lands_in_the_window() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "typed");
    assert!(
        d.timing.turns.is_empty(),
        "precondition: nothing timed yet"
    );

    d.execute_timed(&Intent::Ask("what's outstanding".into()), "what's outstanding");

    assert_eq!(
        d.timing.turns.len(),
        1,
        "the typed prompt must record a turn — an unfilled window reads as a fast one"
    );
    let t = &d.timing.turns[0];
    assert!(
        t.get(Stage::Doing).is_some(),
        "the work is the one stage a typed turn genuinely has"
    );
    assert!(
        !t.about.is_empty(),
        "a slow turn you cannot recognise later is not much use"
    );
}

#[test]
fn a_typed_turn_has_no_speaking_stage_rather_than_a_zero_one() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "typed-absent");
    d.execute_timed(&Intent::Ask("hello".into()), "hello");

    let t = &d.timing.turns[0];
    for absent in [
        Stage::Listening,
        Stage::Hearing,
        Stage::Speaking,
        Stage::Playing,
    ] {
        assert_eq!(
            t.get(absent),
            None,
            "{absent:?} did not happen in a typed turn; recording it as 0ms would make a \
             skipped stage look instant"
        );
    }
}

#[test]
fn a_spoken_exchange_records_every_stage_the_ears_and_mouth_can_split() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "spoken");
    let ears = SplitEars {
        listening_ms: 1_400,
        hearing_ms: 900,
        followups: Cell::new(0),
    };
    let mouth = SplitMouth {
        speaking_ms: 700,
        playing_ms: 2_500,
    };

    d.converse("what's outstanding", &ears, &mouth, &|| 1_000);

    assert_eq!(d.timing.turns.len(), 1, "one exchange, one timed turn");
    let t = &d.timing.turns[0];
    assert_eq!(t.get(Stage::Listening), Some(1_400));
    assert_eq!(t.get(Stage::Hearing), Some(900));
    assert_eq!(t.get(Stage::Speaking), Some(700));
    assert_eq!(t.get(Stage::Playing), Some(2_500));
    assert!(t.get(Stage::Doing).is_some());
}

#[test]
fn waiting_for_you_and_playing_audio_are_not_counted_as_atlas_being_slow() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "not-atlas");
    let ears = SplitEars {
        listening_ms: 9_000,
        hearing_ms: 100,
        followups: Cell::new(0),
    };
    let mouth = SplitMouth {
        speaking_ms: 100,
        playing_ms: 9_000,
    };

    d.converse("a very long question", &ears, &mouth, &|| 1_000);

    let t = &d.timing.turns[0];
    assert!(
        t.atlas_ms() < 1_000,
        "a long question and a long answer are not a slow assistant; atlas_ms was {}",
        t.atlas_ms()
    );
}

#[test]
fn ears_that_cannot_split_leave_the_stages_out_rather_than_inventing_them() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "no-split");

    d.converse("what's outstanding", &MuteEars, &MuteMouth, &|| 1_000);

    let t = &d.timing.turns[0];
    assert_eq!(t.get(Stage::Listening), None);
    assert_eq!(t.get(Stage::Hearing), None);
    assert_eq!(t.get(Stage::Speaking), None);
    assert_eq!(t.get(Stage::Playing), None);
    assert!(
        t.get(Stage::Doing).is_some(),
        "the work is measured here regardless — it is the one part that never \
         depends on the implementation being able to split itself"
    );
}

#[test]
fn no_turns_and_no_slow_turns_do_not_give_the_same_answer() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "empty-vs-fast");

    let nothing_recorded = d.timing.why_slow();

    d.execute_timed(&Intent::Ask("hello".into()), "hello");
    let one_fast_turn = d.timing.why_slow();

    assert_ne!(
        nothing_recorded, one_fast_turn,
        "an unmeasured Atlas must not answer the same as a fast one — that \
         equivalence is the entire bug this file exists for"
    );
}

#[test]
fn a_long_utterance_is_shortened_without_slicing_through_a_character() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "utf8");
    // Accented text and an em dash: byte-offset truncation panics on these,
    // which is how `http.rs` and `faithful.rs` were both crashing.
    let said = "résumé — please find the very long thing about the naïve café budget spreadsheet";
    d.execute_timed(&Intent::Ask(said.into()), said);

    let t = &d.timing.turns[0];
    assert!(t.about.chars().count() <= 49, "kept short");
    assert!(t.about.starts_with("résumé"));
}

#[test]
fn a_spoken_reply_is_not_interrupted_by_recordings_between_its_sentences() {
    // 27 Sep 2026: `say_interruptibly` was handed a one-second listen that
    // recorded and ran speech-to-text before every sentence and after the
    // last -- a reply of five sentences cost six recordings before you had
    // heard it all. Cutting in is by holding the talk key now; with no key
    // held nothing is recorded, and the only listen left is the follow-up
    // that keeps the floor open after the reply.
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "no-listen-between");
    let ears = SplitEars { listening_ms: 10, hearing_ms: 10, followups: Cell::new(0) };
    let mouth = SplitMouth { speaking_ms: 1, playing_ms: 1 };

    d.converse("what's outstanding", &ears, &mouth, &|| 1_000);

    assert_eq!(
        ears.followups.get(),
        1,
        "the ears were asked to listen {} times for one reply; only the follow-up should listen",
        ears.followups.get()
    );
}
