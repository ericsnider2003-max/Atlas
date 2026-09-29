//! Saying "I'm back", and what it is worth.
//!
//! The decision this file defends is one sentence: **the phrase summons the
//! prompt, and the passphrase does the rest.** Everything here is an attempt
//! to catch the phrase doing more than that.
//!
//! Why it is worth this much test: the obvious way to build a spoken way out
//! of a handover is to make the phrase mean something — recognise the owner's
//! voice, accept a spoken passphrase, trust it because the room sounded
//! right. Each of those turns the way back into something a person standing
//! in the room can do, which is precisely the person it is being kept from.
//! The design instead makes the phrase worth *nothing* on its own, and the
//! only way to be sure of that is to keep saying it and check nothing moved.
//!
//! Two pieces of scaffolding, both load-bearing:
//!
//! * `ATLAS_HOME` is set once per process, before anything touches `roots`.
//!   The daemon reads the handover from the *install's* state, so without
//!   this these tests would be reading — and writing — this checkout's own
//!   `data/state`, and a handed-over flag left there would follow the
//!   repository around. `roots` caches its answer in a `OnceLock`, so a
//!   second value would be silently ignored rather than reported.
//! * One lock, held by every test. They share one install by construction,
//!   and `cargo test` runs them on threads; without it, a test asserting the
//!   handover survived would be racing another that ends it.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::handover::{Handover, Stance};
use atlas::intent::Intent;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use atlas::typed::AsksQuietly;
use atlas::vault::{Vault, VaultConfig};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, Once};

const PASSPHRASE: &str = "the one thing you actually know";

/// One install, one test at a time.
fn alone() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    // A test that fails inside the guard poisons it. That is the failure
    // being reported already; re-reporting it as a panic in every following
    // test would bury the one that matters.
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// The install this file pretends to be. Set before `roots` is first asked.
fn home() -> PathBuf {
    static ONCE: Once = Once::new();
    let p = std::env::temp_dir().join("atlas-saying-youre-back");
    ONCE.call_once(|| {
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(p.join("data").join("state")).unwrap();
        std::env::set_var("ATLAS_HOME", &p);
    });
    p
}

/// The install's own state — the same `Store` the daemon will open.
fn install() -> Store {
    home();
    atlas::roots::install_state()
}

/// A vault with a passphrase genuinely set, saved where the daemon reads it.
///
/// Two opens, as `handover.rs`'s own tests do: the first *chooses* the
/// passphrase and proves nothing, the second verifies against it. A fixture
/// that stopped after the first would be setting up the exact case this
/// feature refuses, while reading as though it set up the normal one.
fn a_vault_with_a_passphrase() {
    let mut v = Vault::default();
    v.open(PASSPHRASE, 100, &VaultConfig::default()).expect("first open sets it");
    assert!(v.has_a_passphrase());
    v.lock();
    v.open(PASSPHRASE, 100, &VaultConfig::default()).expect("second open verifies");
    assert!(v.proved_it(), "the fixture's vault cannot prove anything");
    v.lock();
    v.save(&install()).expect("the daemon loads this from disk");
}

fn handed_over(note: &str) {
    let mut h = Handover::default();
    h.hand_over(note, 1_000);
    h.save(&install()).unwrap();
}

fn on_disk() -> Handover {
    Handover::load(&install())
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
    let dir = home().join(format!("person-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Daemon::new(c, p, None, Store::new(dir), Proactive::new(ProactiveConfig::default()))
}

/// A keyboard that types whatever it was told to, and remembers being asked.
///
/// The record is shared rather than owned, because the daemon takes the box
/// and the test still has to see what was put in front of the person.
#[derive(Clone)]
struct Types {
    answer: Option<String>,
    asked: Arc<Mutex<Vec<String>>>,
}

impl Types {
    fn nothing() -> Self {
        Types { answer: None, asked: Arc::new(Mutex::new(Vec::new())) }
    }
    fn saying(what: &str) -> Self {
        Types { answer: Some(what.to_string()), asked: Arc::new(Mutex::new(Vec::new())) }
    }
    fn prompts(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

impl AsksQuietly for Types {
    fn ask(&self, prompt: &str) -> Option<String> {
        self.asked.lock().unwrap().push(prompt.to_string());
        self.answer.clone()
    }
}

// --- the phrase alone ------------------------------------------------------

#[test]
fn the_phrase_carries_nothing_that_could_be_a_passphrase() {
    // The shape of the intent is the guarantee. `TakeItBack` has no field,
    // so there is nowhere for a spoken secret to arrive even if some future
    // branch wanted to read one -- and "im back hunter2" parses to exactly
    // the same value as "im back", with the rest of the sentence dropped by
    // the parser rather than by a rule somebody has to remember.
    let parser = atlas::intent::Parser::new(&cfg().commands);
    assert_eq!(parser.parse("this is mine again"), Intent::TakeItBack);
    assert_eq!(
        parser.parse("hand it back the one thing you actually know"),
        Intent::TakeItBack,
        "a spoken passphrase changed what the phrase parsed to"
    );
    assert_eq!(parser.parse("end the handover"), Intent::TakeItBack);

    // And the one that is not this command's to claim. "im back" has been
    // `resume`'s since long before a handover existed, and a second command
    // listing it would have made one of the two unreachable -- silently,
    // because the parser resolves ties by position in the file. It reaches
    // the same prompt anyway, through the daemon, which the next test shows.
    assert_eq!(parser.parse("im back"), Intent::Resume);
    // And one more that was already spoken for: "take it back" is
    // `history`'s, and means undo the change you just made.
    assert_ne!(parser.parse("take it back"), Intent::TakeItBack);
}

#[test]
fn the_way_out_is_not_itself_refused_while_handed_over() {
    // The mistake that would close the door: adding `take_it_back` to a
    // refusal list. Every other name on them is something a stranger must
    // not do; this one is the only thing they might need to do, because the
    // person taking the laptop back says the phrase and then types.
    assert!(
        !atlas::handover::refuses("take_it_back"),
        "the phrase that ends a handover is refused while handed over -- \
         there is now no way back except the command line"
    );
}

// --- summoning the prompt --------------------------------------------------

#[test]
fn im_back_reaches_the_prompt_even_though_the_phrase_belongs_to_resume() {
    // The bridge, tested through the daemon rather than the parser, because
    // the parser is not where it lives: "im back" parses to `Resume`, and a
    // `Resume` with nothing paused and the machine handed over is the owner
    // at the desk. Without this, the most natural sentence anyone would say
    // gets "Wasn't paused." and the laptop stays handed over.
    let _lock = alone();
    a_vault_with_a_passphrase();
    handed_over("Sam has it");
    let (c, p) = (cfg(), plat());
    let keyboard = Types::saying(PASSPHRASE);
    let mut d = daemon(&c, &p, "bridge").with_typed_prompt(Box::new(keyboard.clone()));

    // The sentence itself, not a synonym. An earlier version of this file
    // said "this is mine again" here after a bulk rename, and passed with
    // the bridge deleted -- it was testing the command that does not need a
    // bridge. Caught by planting the deletion and watching this stay green.
    let said = d.turn("im back", 2_000);
    assert_eq!(keyboard.prompts().len(), 1, "it did not ask for the passphrase: {said}");
    assert!(said.contains("Yours again"), "{said}");
    assert_eq!(on_disk().stance, Stance::Yours);
}

#[test]
fn resume_still_resumes_when_something_is_actually_paused() {
    // The other side of the bridge. A guest who paused Atlas and said "carry
    // on" must get their pause back, not a passphrase prompt -- the handover
    // reading is only for the case where there is nothing to resume.
    let _lock = alone();
    a_vault_with_a_passphrase();
    handed_over("");
    let (c, p) = (cfg(), plat());
    let keyboard = Types::saying(PASSPHRASE);
    let mut d = daemon(&c, &p, "paused").with_typed_prompt(Box::new(keyboard.clone()));

    d.turn("hold on", 2_000);
    let said = d.turn("carry on", 2_100);
    assert!(keyboard.prompts().is_empty(), "a paused Atlas asked for the passphrase: {said}");
    assert_eq!(on_disk().stance, Stance::HandedOver, "resuming ended the handover");
}

#[test]
fn saying_it_asks_for_the_passphrase_and_changes_nothing_by_itself() {
    let _lock = alone();
    a_vault_with_a_passphrase();
    handed_over("Sam has it");
    let (c, p) = (cfg(), plat());
    let keyboard = Types::nothing();
    let mut d = daemon(&c, &p, "nothing").with_typed_prompt(Box::new(keyboard.clone()));

    let said = d.turn("this is mine again", 2_000);
    assert_eq!(keyboard.prompts().len(), 1, "it did not ask for the passphrase");
    assert!(said.contains("Nothing typed"), "{said}");
    assert_eq!(
        on_disk().stance,
        Stance::HandedOver,
        "the phrase ended the handover without a passphrase"
    );
    assert_eq!(on_disk().refused, 0, "walking away from the prompt counted against someone");
}

#[test]
fn a_wrong_passphrase_leaves_it_handed_over_and_is_counted() {
    let _lock = alone();
    a_vault_with_a_passphrase();
    handed_over("");
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "wrong")
        .with_typed_prompt(Box::new(Types::saying("not the passphrase at all")));

    let said = d.turn("this is mine again", 2_000);
    assert!(!said.contains("Yours again"), "a wrong passphrase was accepted: {said}");
    assert_eq!(on_disk().stance, Stance::HandedOver);
    assert_eq!(on_disk().refused, 1, "the attempt was not counted");
}

#[test]
fn the_right_passphrase_typed_ends_it() {
    // The other half. A gate that never opens is not a gate, it is a wall,
    // and the way this feature fails quietly is by being impossible to pass
    // -- at which point nobody uses it and the machine stays handed over.
    let _lock = alone();
    a_vault_with_a_passphrase();
    handed_over("Sam has it");
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "right").with_typed_prompt(Box::new(Types::saying(PASSPHRASE)));

    let said = d.turn("this is mine again", 2_000);
    assert!(said.contains("Yours again"), "{said}");
    assert_eq!(on_disk().stance, Stance::Yours);
    assert!(on_disk().note.is_empty(), "the note outlived the handover");
}

#[test]
fn the_prompt_explains_itself_because_nothing_can_speak_before_it() {
    // The prompt blocks, so Atlas cannot say "check the terminal" and then
    // ask -- the person is looking at a screen that has just grown one line.
    // A bare "Passphrase:" is how somebody types their email password into
    // the wrong window.
    let _lock = alone();
    a_vault_with_a_passphrase();
    handed_over("");
    let (c, p) = (cfg(), plat());
    let keyboard = Types::nothing();
    let mut d = daemon(&c, &p, "wording").with_typed_prompt(Box::new(keyboard.clone()));
    d.turn("this is mine again", 2_000);

    let prompt = keyboard.prompts().first().cloned().expect("nothing was put in front of anyone");
    assert!(prompt.contains("Atlas"), "the prompt does not say who is asking: {prompt}");
    assert!(prompt.contains("passphrase"), "the prompt does not say what it wants: {prompt}");
    assert!(
        prompt.contains("won't be shown"),
        "the prompt does not say the typing is hidden, so a dead-looking screen \
         reads as a hang: {prompt}"
    );
}

#[test]
fn with_nowhere_to_type_it_says_so_rather_than_appearing_to_have_asked() {
    // The default every test gets and every headless run gets: no console of
    // Atlas's own. The honest answer names the command that has one. The
    // dishonest one -- which is what a `unwrap_or_default()` here would
    // produce -- is "nothing typed", from a prompt that never existed.
    let _lock = alone();
    a_vault_with_a_passphrase();
    handed_over("");
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "nowhere");

    let said = d.turn("this is mine again", 2_000);
    // 27 Sep 2026: the way that works is now the Accounts page's "Take it
    // back" box, not the terminal command -- the person this is said to may
    // never open a terminal. Still asserted: it names somewhere real.
    assert!(
        said.contains("Accounts page") && said.contains("Vault"),
        "with nowhere to type, it did not name the way that works: {said}"
    );
    assert!(!said.contains("atlas handover"), "a terminal command in a spoken answer: {said}");
    assert!(!said.contains("Nothing typed"), "it claimed to have asked: {said}");
    assert_eq!(on_disk().stance, Stance::HandedOver);
}

#[test]
fn saying_it_when_nothing_was_handed_over_is_not_an_error() {
    let _lock = alone();
    a_vault_with_a_passphrase();
    Handover::default().save(&install()).unwrap();
    let (c, p) = (cfg(), plat());
    let keyboard = Types::saying(PASSPHRASE);
    let mut d = daemon(&c, &p, "already").with_typed_prompt(Box::new(keyboard.clone()));

    let said = d.turn("this is mine again", 2_000);
    assert!(said.contains("yours already"), "{said}");
    assert!(
        keyboard.prompts().is_empty(),
        "it asked for a passphrase it had no use for"
    );
}

#[test]
fn with_no_passphrase_on_the_vault_it_says_so_instead_of_asking_for_one() {
    // Asking somebody to type a passphrase that cannot prove anything is
    // worse than refusing: they type it, it "works", and the lock has opened
    // for a string chosen by whoever typed first.
    let _lock = alone();
    let mut fresh = Vault::default();
    fresh.lock();
    fresh.save(&install()).unwrap();
    assert!(!Vault::load(&install()).has_a_passphrase());
    handed_over("");

    let (c, p) = (cfg(), plat());
    let keyboard = Types::saying("twelve characters at least");
    let mut d = daemon(&c, &p, "nopass").with_typed_prompt(Box::new(keyboard.clone()));

    let said = d.turn("this is mine again", 2_000);
    assert!(said.contains("no passphrase"), "{said}");
    // 27 Sep 2026: named `atlas vault` until the Accounts page could set a
    // passphrase; now it points there.
    assert!(said.contains("Accounts page"), "it does not say how to fix it: {said}");
    assert!(keyboard.prompts().is_empty(), "it asked anyway");
    assert_eq!(on_disk().stance, Stance::HandedOver);
}

