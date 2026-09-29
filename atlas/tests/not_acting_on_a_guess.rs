//! Not acting on a guess.
//!
//! Atlas understands you two ways. A phrase in its own list is matched
//! outright. Anything else goes to a small local model, which returns an
//! intent in the same confident shape whether it recognised the sentence or
//! invented a reading of it.
//!
//! `policy::classify` grades *what the action is*. Nothing told it *how sure
//! Atlas was that you asked for it*, so both readings were graded the same
//! and both simply happened. The only thing standing between a misheard
//! sentence and a real action was a line in the model's own prompt asking it
//! to "use ask rather than guessing" — an instruction to the very model whose
//! confident wrongness `certainty.rs` exists to catch, with nothing checking
//! whether it obeyed.
//!
//! What these tests pin is the third option: not guessing, and not refusing
//! either. Atlas says what it took you to mean and waits for an answer.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::brain::Llm;
use atlas::error::Result;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-guess-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, llm: Option<Arc<dyn Llm>>, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, llm, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

/// A model that confidently reads any unrecognised sentence as "rebuild the
/// notes index" — an action graded `ProceedAndReport`, meaning it happens and
/// then tells you.
struct ConfidentlyWrong;
impl Llm for ConfidentlyWrong {
    fn complete(&self, _: &str, _: &str) -> Result<String> {
        Ok(r#"{"action":"rebuild_index","arg":null,"say":"Rebuilding the index."}"#.into())
    }
}

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}

// ---------- the gap itself ----------

#[test]
fn an_inferred_action_is_put_to_you_before_it_happens() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, Some(Arc::new(ConfidentlyWrong)), "inferred");

    // Deliberately not a phrase Atlas knows, so it reaches the model.
    let reply = d.turn("mmh the thing with the files would you", 100);

    // Asserted against what *running* the action actually says, not against
    // the model's own `say`. The first draft of this test checked for the
    // model's sentence, which never appears either way -- so it passed with
    // the gate torn out, which is how it was caught.
    assert!(
        !reply.contains("nothing to index"),
        "ran the action on the model's reading without checking: {reply}"
    );
    assert!(reply.contains("I took that as"), "did not put it to you: {reply}");
}

#[test]
fn it_says_what_it_took_you_to_mean() {
    // The whole point of the third option. "I didn't catch that" throws away
    // a reading Atlas actually made and makes you start the sentence over;
    // naming the reading lets you correct one word.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, Some(Arc::new(ConfidentlyWrong)), "names-it");

    let reply = d.turn("mmh the thing with the files would you", 100);

    assert!(
        reply.contains("rebuilding the notes index"),
        "did not name the reading it made: {reply}"
    );
    assert!(
        !reply.to_lowercase().contains("didn't catch"),
        "fell back to pleading ignorance instead of stating the assumption: {reply}"
    );
}

#[test]
fn a_matched_phrase_is_not_second_guessed() {
    // The noise guard, and the reason this is keyed to *how* Atlas understood
    // rather than to the action. A phrase Atlas matches outright cannot have
    // been misread, so asking about it would be pure friction.
    //
    // The same daemon, the same action, the only difference being that the
    // phrase list knew the words.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, Some(Arc::new(ConfidentlyWrong)), "matched");

    let reply = d.turn("rebuild the index", 100);

    assert!(
        reply.contains("nothing to index"),
        "a phrase it matched exactly did not just run: {reply}"
    );
    assert!(
        !reply.contains("I took that as"),
        "asked about a phrase it matched exactly: {reply}"
    );
}

#[test]
fn switching_it_off_restores_the_old_behaviour_exactly() {
    // It is a gate, so it has an off switch, and off must mean off rather
    // than "mostly off".
    let mut c = cfg();
    c.tools.as_mut().expect("tools section").understood.enabled = false;
    let p = plat();
    let mut d = daemon(&c, &p, Some(Arc::new(ConfidentlyWrong)), "off");

    let reply = d.turn("mmh the thing with the files would you", 100);

    assert!(
        reply.contains("nothing to index"),
        "with the gate off the action should simply run: {reply}"
    );
    assert!(!reply.contains("I took that as"), "still gating with the setting off: {reply}");
}

// ---------- the setting is real ----------

#[test]
fn the_gate_ships_on() {
    // Unlike most of tools.yaml, which ships off so that a missing config
    // means Atlas does less rather than more. Switching this one off does not
    // remove a behaviour, it removes a question — and what is left is Atlas
    // acting on the model's guess without saying so. "Less" here means fewer
    // things done unasked.
    let c = cfg();
    assert!(
        c.tools.as_ref().expect("tools.yaml loads").understood.enabled,
        "the guard against acting on a guess ships disabled"
    );
}

#[test]
fn the_setting_exists_in_the_shipped_file() {
    // Not just in the struct's defaults. A setting that only exists as a Rust
    // default is one nobody can find to change.
    let raw = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");
    assert!(raw.contains("understood:"), "no understood block in the shipped config");
}
