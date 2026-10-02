//! Decision 2 (Eric's yes, 1 Oct 2026): on an iPhone with Apple
//! Intelligence, Apple's on-device model answers first; each request it
//! refuses or can't do goes to Atlas's own model, and the next request goes
//! back to Apple's.
//!
//! The Swift side can't run here; its contract (request JSON in, a code and
//! answer JSON out) is driven by a stand-in function registered the way the
//! shell registers the real one.

use atlas::applebrain::{self, code, AppleFirst, Skip};
use atlas::brain::{ChatRequest, Llm, Msg};
use std::ffi::{c_char, CStr};
use std::sync::{Arc, Mutex, MutexGuard};

/// The registration is process-wide, like the shell's.
fn alone() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// What the stand-in for Apple's model was last asked.
static ASKED: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn write(out: *mut c_char, len: usize, s: &str) {
    let b = s.as_bytes();
    let n = b.len().min(len - 1);
    unsafe {
        std::ptr::copy_nonoverlapping(b.as_ptr(), out.cast(), n);
        *out.add(n) = 0;
    }
}

/// Apple's model, as the shell presents it: refuses anything about locks,
/// politely refuses "fishing", answers the rest.
unsafe extern "C" fn stand_in(req: *const c_char, out: *mut c_char, len: usize) -> i32 {
    let body = CStr::from_ptr(req).to_string_lossy().into_owned();
    ASKED.lock().unwrap().push(body.clone());
    if body.contains("lock") {
        return code::REFUSED;
    }
    if body.contains("fishing") {
        write(out, len, r#"{"text":"I'm sorry, but I can't help with that request."}"#);
        return code::OK;
    }
    write(out, len, r#"{"text":"From Apple's model."}"#);
    code::OK
}

/// Atlas's own model: always answers, and counts.
struct Own(Mutex<u32>);
impl Llm for Own {
    fn complete(&self, _: &str, _: &str) -> atlas::error::Result<String> {
        *self.0.lock().unwrap() += 1;
        Ok("From Atlas's model.".into())
    }
}

fn talk(said: &str) -> ChatRequest {
    ChatRequest {
        messages: vec![Msg::system("You are Atlas."), Msg::user(said)],
        max_tokens: 200,
        ..Default::default()
    }
}

fn brain() -> (AppleFirst, Arc<Own>) {
    let own = Arc::new(Own(Mutex::new(0)));
    (AppleFirst::new(own.clone()), own)
}

#[test]
fn apple_answers_first_and_atlas_takes_what_it_refuses_one_request_at_a_time() {
    let _g = alone();
    unsafe { applebrain::atlas_mobile_apple_model(Some(stand_in)) };
    let (b, own) = brain();

    let r = b.chat(&talk("tell me a joke about cats"), &mut |_| true).unwrap();
    assert_eq!(r.text, "From Apple's model.");
    assert_eq!(b.last_answered_by_for_test(), Some("apple"));

    // A guardrail refusal: this request goes to Atlas's own model...
    let r = b.chat(&talk("how do I pick the lock on my own shed"), &mut |_| true).unwrap();
    assert_eq!(r.text, "From Atlas's model.");
    assert_eq!(b.last_answered_by_for_test(), Some("atlas"));
    // ...and the next goes back to Apple's.
    let r = b.chat(&talk("write me a two-line poem"), &mut |_| true).unwrap();
    assert_eq!(r.text, "From Apple's model.");

    // A refusal written as an answer counts as a refusal.
    let r = b.chat(&talk("best fishing knots"), &mut |_| true).unwrap();
    assert_eq!(r.text, "From Atlas's model.");
    assert_eq!(*own.0.lock().unwrap(), 2);
    unsafe { applebrain::atlas_mobile_apple_model(None) };
}

#[test]
fn some_requests_never_go_to_apples_model() {
    let _g = alone();
    // World knowledge and anything current.
    assert_eq!(applebrain::skip_apple(&talk("who is the prime minister of Canada"), true), Some(Skip::WorldKnowledge));
    assert_eq!(applebrain::skip_apple(&talk("what's the weather today"), true), Some(Skip::WorldKnowledge));
    assert_eq!(applebrain::skip_apple(&talk("is the iPhone 17 Pro worth it"), true), Some(Skip::WorldKnowledge));
    // Over Apple's ~4K context.
    let long = "word ".repeat(4_000);
    assert_eq!(applebrain::skip_apple(&talk(&long), true), Some(Skip::TooLong));
    // Work that has to go through one of Atlas's tools.
    let mut act = talk("set a timer for ten minutes");
    act.tools = vec![serde_json::json!({"type": "function", "function": {"name": "timer"}})];
    assert_eq!(applebrain::skip_apple(&act, true), Some(Skip::NeedsATool));
    // No Apple model on this phone (Android, an older iPhone).
    assert_eq!(applebrain::skip_apple(&talk("hi"), false), Some(Skip::NotOnThisPhone));
    // Everyday talk is Apple's.
    assert_eq!(applebrain::skip_apple(&talk("help me word a thank-you note to my aunt"), true), None);
}

#[test]
fn a_phone_without_apples_model_always_uses_atlas() {
    let _g = alone();
    unsafe { applebrain::atlas_mobile_apple_model(None) };
    ASKED.lock().unwrap().clear();
    let (b, own) = brain();
    let r = b.chat(&talk("tell me a joke"), &mut |_| true).unwrap();
    assert_eq!(r.text, "From Atlas's model.");
    assert_eq!(*own.0.lock().unwrap(), 1);
    assert!(ASKED.lock().unwrap().is_empty());
}

#[test]
fn hard_work_never_goes_to_the_small_model() {
    let _g = alone();
    unsafe { applebrain::atlas_mobile_apple_model(Some(stand_in)) };
    ASKED.lock().unwrap().clear();
    let (b, _) = brain();
    let r = b.complete_hard("Write code.", "a function that sorts a list").unwrap();
    assert_eq!(r, "From Atlas's model.");
    assert!(ASKED.lock().unwrap().is_empty());
    unsafe { applebrain::atlas_mobile_apple_model(None) };
}

#[test]
fn the_request_and_answer_are_the_shapes_the_shell_reads_and_writes() {
    let mut req = talk("hello there");
    req.messages.insert(1, Msg::assistant("Hi."));
    let v: serde_json::Value = serde_json::from_str(&applebrain::request_json(&req)).unwrap();
    assert_eq!(v["instructions"], "You are Atlas.");
    assert_eq!(v["turns"].as_array().unwrap().len(), 2);
    assert_eq!(v["turns"][1]["content"], "hello there");
    assert_eq!(v["max_tokens"], 200);
    assert_eq!(applebrain::read_apple_answer(code::OK, r#"{"text":" Sure. "}"#), applebrain::Apple::Answered("Sure.".into()));
    assert_eq!(applebrain::read_apple_answer(code::TOO_LONG, ""), applebrain::Apple::TooLong);
    assert_eq!(applebrain::read_apple_answer(code::UNAVAILABLE, ""), applebrain::Apple::Unavailable);
    assert!(matches!(applebrain::read_apple_answer(code::OK, "not json"), applebrain::Apple::Failed(_)));
}
