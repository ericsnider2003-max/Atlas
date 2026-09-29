//! Eric's rulings of 25 Sep 2026: D (Atlas asking a bigger model for help,
//! minimally), I (video and creator advice, kept), J (money advice, kept,
//! general only), H5 (other languages) and H6 (teaching a gesture).

use atlas::brain::Llm;
use atlas::build_it::{Check, Outcome, Struggle};
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::error::Result;
use atlas::handshape::{Landmarks, Point, Vocabulary, POINTS};
use atlas::intent::Intent;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-advice-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn cfg() -> &'static Config {
    Box::leak(Box::new(Config::load(Path::new("config")).unwrap()))
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(scratch(tag)), Proactive::new(ProactiveConfig::default()))
}

// ------------------------------------------------------------------ D

/// A stronger model that answers the write-up with a working program.
struct Bigger {
    saw: std::sync::Mutex<String>,
}

impl Llm for Bigger {
    fn complete(&self, _s: &str, _u: &str) -> Result<String> {
        Ok("I'd guess".into())
    }
    fn has_stronger(&self) -> bool {
        true
    }
    fn complete_hard(&self, _s: &str, brief: &str) -> Result<String> {
        *self.saw.lock().unwrap() = brief.to_string();
        Ok("The loop never ends. src/main.rs:\n```rust\nfn main() { works }\n```".into())
    }
}

fn stuck() -> Struggle {
    Struggle {
        description: "rename files by date".into(),
        lang: atlas::craft::Lang::Rust,
        code: "fn main() { broken }".into(),
        failure: "error[E0425]: cannot find value `broken`".into(),
    }
}

#[test]
fn a_stuck_build_is_written_up_and_the_bigger_models_answer_is_checked_here() {
    let llm = Bigger { saw: std::sync::Mutex::new(String::new()) };
    let cfg = atlas::handoff::HandoffConfig::default();
    let (outcome, said) = atlas::build_it::ask_for_help(&stuck(), 12, &llm, &cfg, |code: &str| {
        if code.contains("works") {
            Check::Passed(vec![])
        } else {
            Check::Failed("still broken".into())
        }
    });
    assert!(matches!(outcome, Outcome::Built { rounds: 13, .. }), "{outcome:?}");
    assert!(said.contains("checks out here"), "{said}");
    let brief = llm.saw.lock().unwrap().clone();
    assert!(brief.contains("rename files by date") && brief.contains("E0425") && brief.contains("fn main() { broken }"), "{brief}");

    // Its answer is a draft until it passes here.
    let (outcome, said) = atlas::build_it::ask_for_help(&stuck(), 12, &llm, &cfg, |_: &str| Check::Failed("nope".into()));
    assert!(matches!(outcome, Outcome::Struggled { .. }), "{outcome:?}");
    assert!(said.contains("kept it as a draft"), "{said}");
}

#[test]
fn only_a_configured_stronger_model_counts_as_one() {
    struct One;
    impl Llm for One {
        fn complete(&self, _s: &str, _u: &str) -> Result<String> {
            Ok(String::new())
        }
    }
    assert!(!One.has_stronger());
    let both = atlas::brain::FallbackLlm::new(std::sync::Arc::new(One), Some(std::sync::Arc::new(One)));
    assert!(both.has_stronger());
    let alone = atlas::brain::FallbackLlm::new(std::sync::Arc::new(One), None);
    assert!(!alone.has_stronger());
}

// ------------------------------------------------------------------ I

#[test]
fn a_deal_that_asks_without_paying_is_called_what_it_is() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(c, &p, "deal");
    let said = d.execute(&Intent::CreatorAdvice(
        "is this deal good: 15% commission, exclusive in the camera category, 2 posts a month and a link in your bio".into(),
    ));
    assert!(said.contains("isn't one") && said.contains("15%"), "{said}");
    assert!(said.contains("exclusivity") && said.contains("a bio placement"), "{said}");
    let said = d.execute(&Intent::CreatorAdvice("is this a good deal: 10% affiliate link, i already use it".into()));
    assert!(said.starts_with("That's a genuine affiliate arrangement"), "{said}");
}

#[test]
fn grading_formats_and_the_profile_are_answered_from_what_atlas_knows() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(c, &p, "creator");
    let said = d.execute(&Intent::CreatorAdvice("colour grading steps".into()));
    assert!(said.starts_with("Always the same nodes, in this order:\n1."), "{said}");
    assert!(said.contains("contrast pivot 0.336"), "{said}");

    let said = d.execute(&Intent::CreatorAdvice("export settings for tiktok, it's a talking head".into()));
    assert!(said.contains("For TikTok: 1080x1920") && said.contains("your face fills the frame"), "{said}");

    let said = d.execute(&Intent::CreatorAdvice("my creator profile has a photo and my name".into()));
    assert!(said.starts_with("2 of 5.") && said.contains("who this is for"), "{said}");
}

#[test]
fn creator_advice_follows_its_switch() {
    let mut c = Config::load(Path::new("config")).unwrap();
    assert!(c.tools.as_ref().unwrap().editcraft.enabled, "shipped on, by Eric's ruling I");
    c.tools.as_mut().unwrap().editcraft.enabled = false;
    let c: &'static Config = Box::leak(Box::new(c));
    let p = plat();
    let mut d = daemon(c, &p, "creator-off");
    assert!(d.execute(&Intent::CreatorAdvice("colour grading steps".into())).contains("switched off"));
}

// ------------------------------------------------------------------ J

#[test]
fn money_questions_get_the_general_rule_and_say_so() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(c, &p, "money");
    let said = d.execute(&Intent::MoneyAdvice("how long should i keep receipts".into()));
    assert!(said.starts_with("Generally three years") && said.ends_with("not advice for your situation."), "{said}");
    let said = d.execute(&Intent::MoneyAdvice("would you need my bank login".into()));
    assert!(said.contains("a file you exported yourself") && said.contains("I'd hold your login"), "{said}");
    let first_safe = said.find("a file you exported").unwrap();
    let login = said.find("your bank's own download button").unwrap();
    assert!(first_safe < login, "the ways that never hold your login come first");
}

// ------------------------------------------------------------------ H5

#[test]
fn which_languages_it_hears_is_said_from_the_model_it_has() {
    let mut c = Config::load(Path::new("config")).unwrap();
    c.tools.as_mut().unwrap().vars.insert("stt_model".into(), "models/ggml-base.en.bin".into());
    let c: &'static Config = Box::leak(Box::new(c));
    let p = plat();
    let mut d = daemon(c, &p, "lang");
    let said = d.execute(&Intent::Languages(String::new()));
    assert!(said.starts_with("Only English right now.") && said.contains("English-only"), "{said}");
}

#[test]
fn mishearing_often_is_said_once_with_what_would_help() {
    let lc = atlas::language::LanguageConfig::default();
    let mut l = atlas::language::Listening::default();
    for _ in 0..lc.struggles_before_suggesting {
        l.record(0.3);
    }
    let s = l.suggestion("ggml-base.en.bin", &lc).expect("struggling is said");
    assert!(s.contains("mishearing"), "{s}");
    l.suggested_at = Some(1);
    assert!(l.suggestion("ggml-base.en.bin", &lc).is_none(), "once");
}

// ------------------------------------------------------------------ H6

fn fist() -> Landmarks {
    let mut points = [Point::default(); POINTS];
    points[0] = Point::from(0.5, 0.9);
    points[9] = Point::from(0.5, 0.7);
    for (i, k) in [2usize, 5, 9, 13, 17].iter().enumerate() {
        points[*k] = Point::from(0.44 + i as f32 * 0.03, 0.72);
    }
    for (i, t) in [4usize, 8, 12, 16, 20].iter().enumerate() {
        points[*t] = Point::from(0.44 + i as f32 * 0.03, 0.73);
    }
    Landmarks { points, right: Some(true), sure: 0.95 }
}

/// A camera that sees the same hand every time: index and little finger up.
struct Shown(Landmarks);
impl atlas::handloop::Eyes for Shown {
    fn look(&mut self) -> Option<Landmarks> {
        Some(self.0.clone())
    }
}

fn horns() -> Landmarks {
    let mut h = fist();
    let span = h.span();
    for (k, t) in [(5usize, 8usize), (17, 20)] {
        let knuckle = h.points[k];
        h.points[t] = Point::from(knuckle.x, knuckle.y - span * 1.1);
    }
    h
}

#[test]
fn a_gesture_shown_to_the_camera_is_learned_and_adopted() {
    assert_eq!(
        atlas::daemon::gesture_asked("teach you a gesture called rock on that opens spotify"),
        Some(("rock on".to_string(), "open spotify".to_string()))
    );
    let g = atlas::handloop::shape_shown(&mut Shown(horns()), "rock on", "open spotify", 100).unwrap();
    assert_eq!((g.name.as_str(), g.does.as_str()), ("rock on", "open spotify"));
    let mut v = Vocabulary::default();
    let said = v.adopt(g.clone()).unwrap();
    assert!(said.starts_with("Got it."), "{said}");
    assert!(v.adopt(g).is_err(), "not twice under one name");

    // Nothing in view: said, not guessed.
    struct Nobody;
    impl atlas::handloop::Eyes for Nobody {
        fn look(&mut self) -> Option<Landmarks> {
            None
        }
    }
    assert_eq!(atlas::handloop::shape_shown(&mut Nobody, "x", "y", 50).unwrap_err(), "I didn't see your hand at all.");
}

#[test]
fn teaching_with_the_camera_off_says_so() {
    let mut c = Config::load(Path::new("config")).unwrap();
    c.tools.as_mut().unwrap().gaze.enabled = false;
    let c: &'static Config = Box::leak(Box::new(c));
    let p = plat();
    let mut d = daemon(c, &p, "teach-off");
    let said = d.execute(&Intent::TeachGesture("teach you a gesture called rock on that opens spotify".into()));
    assert!(said.contains("camera's switched off"), "{said}");
}

#[test]
fn a_shape_is_watched_until_there_are_enough_readings_and_no_longer() {
    struct Counting(Landmarks, usize);
    impl atlas::handloop::Eyes for Counting {
        fn look(&mut self) -> Option<Landmarks> {
            self.1 += 1;
            Some(self.0.clone())
        }
    }
    let mut eyes = Counting(horns(), 0);
    let l = atlas::handloop::learn_a_shape(&mut eyes, 500);
    assert!(l.enough());
    assert_eq!(eyes.1, atlas::handshape::SAMPLES, "stops as soon as it has enough");
}
