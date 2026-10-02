//! Atlas running as the iPhone or Android app (2 Oct 2026, the TestFlight
//! review audit): the same hub, without what only makes sense on a laptop.
//!
//! On a phone there is no "put Atlas on your phone" page, no self-update
//! (the store or TestFlight updates the app; Apple doesn't allow an app to
//! install new versions of itself), no syncing laptops, offline packs,
//! gestures through a camera, add-on installs, PC tune-up or folder
//! sorting -- and nothing large downloads, and nothing goes to an online
//! model, until you've said yes.
//!
//! `mobile::serve` switches it on for the whole app. A test switches it on
//! for its own thread only, so tests running beside it see a laptop.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

static ON: AtomicBool = AtomicBool::new(false);
/// You said yes to the free online models, on the phone.
static ONLINE: AtomicBool = AtomicBool::new(false);

thread_local! {
    static HERE: Cell<Option<bool>> = const { Cell::new(None) };
}

/// Is this Atlas the phone app?
pub fn on() -> bool {
    HERE.with(|h| h.get()).unwrap_or_else(|| ON.load(Ordering::Relaxed))
}

/// The phone app, from now on, for every thread (`mobile::serve`).
pub fn switch_on() {
    ON.store(true, Ordering::Relaxed);
}

/// The phone app (or not) on this thread only, for the tests.
pub fn on_this_thread_for_test(phone: bool) {
    HERE.with(|h| h.set(Some(phone)));
}

/// The store record that says you asked for the phone's own model: until
/// it's there, nothing large downloads by itself.
pub const MODEL_ASKED_FOR: &str = "phone_model_asked_for";

/// The store record of your yes (or no) to the free online models.
pub const ONLINE_ASKED: &str = "phone_online_models";

/// May a question go to the free online models from this phone?
pub fn online_ok() -> bool {
    ONLINE.load(Ordering::Relaxed)
}

/// Your answer, for this run (`mobile::serve` reads it back at the start).
pub fn set_online_ok(yes: bool) {
    ONLINE.store(yes, Ordering::Relaxed);
}

/// What the free online models answer before your yes: the question, put
/// to you, with what each answer means. Apple's review asks exactly this
/// (5.1.2): say who gets what, and ask first.
pub const ASK_ONLINE: &str = "To answer that I need a language model, and this phone hasn't got one of its own yet. \
     I can send your questions to free online AI services (Kilo, Pollinations and OVHcloud) -- they see what you ask, \
     nothing else. Say \"use online models\" for that, or \"get your own model\" to fetch one onto this phone \
     (0.6 to 1.8 GB, on Wi-Fi) so nothing leaves it.";

/// "use online models" / "stop using online models": your answer, or `None`
/// when that isn't what was said.
pub fn online_answer(said: &str) -> Option<bool> {
    let t = said.trim().trim_end_matches(['.', '!']).to_ascii_lowercase();
    let t = t.strip_prefix("yes, ").or_else(|| t.strip_prefix("yes ")).unwrap_or(&t).to_string();
    match t.as_str() {
        "use online models" | "use the online models" | "use free online models" | "allow online models" => Some(true),
        "stop using online models" | "don't use online models" | "dont use online models" | "no online models" => Some(false),
        _ => None,
    }
}

/// What a model question gets on a phone that has no model yet.
pub const NO_MODEL_YET: &str = "This phone has no language model of its own yet, so I can't answer that one. \
     Say \"get your own model\" and I'll fetch one (0.6 to 1.8 GB, on Wi-Fi only) -- reminders, notes, your calendar and the rest work already.";

/// The settings as the phone app starts with them: no trading check-ins
/// nobody asked for. (The free online models stay, behind your yes --
/// `online_ok`.)
pub fn as_a_phone(t: &mut crate::voice::ToolsConfig) {
    t.workday.trade_day.enabled = false;
    t.workday.trade_day.prompt = false;
}

/// Which models this phone answers with, in a sentence, for the self-check.
pub fn models_said() -> String {
    let apple = if crate::applebrain::registered() { "Apple's model is here and answers first" } else { "Apple's model isn't available on this phone" };
    #[cfg(feature = "phone-llm")]
    let own = match crate::phonemodel::attached() {
        Some(name) => format!("its own model, {name}, is loaded"),
        None => "it has no model of its own yet (say \"get your own model\")".to_string(),
    };
    #[cfg(not(feature = "phone-llm"))]
    let own = "this build has no model of its own".to_string();
    let online = if online_ok() { "the free online models are allowed" } else { "nothing goes to the online models" };
    format!("{apple}; {own}; {online}.")
}
