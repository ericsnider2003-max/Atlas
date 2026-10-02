//! Apple's on-device model as the iPhone's first brain, with Atlas's own
//! model behind it for each request it can't do (decision 2, Eric's yes,
//! 1 Oct 2026).
//!
//! Where the iPhone has Apple Intelligence, Apple's model
//! (`SystemLanguageModel.default`, FoundationModels, iOS 26) answers first:
//! it's already on the phone, costs no download and no memory of Atlas's
//! own. It is small (~3B), its context is about 4,096 tokens, it is "not
//! designed to be a chatbot for general world knowledge" (Apple's own
//! documentation), and its guardrails refuse some ordinary requests. So each
//! request is judged on its own:
//!
//! - **Straight to Atlas's model**, without asking Apple's: a request over
//!   Apple's context; a question needing world knowledge or anything current
//!   (`freshness::shelf_for`); a request that has to act through one of
//!   Atlas's tools (Apple's model doesn't call Atlas's tools).
//! - **Asked of Apple's, then Atlas's** when Apple's refuses: a guardrail or
//!   refusal error from the framework, its context exceeded after all, or a
//!   soft refusal in the reply text ("I can't help with that").
//! - **The next request goes back to Apple's** -- nothing is remembered
//!   between requests.
//!
//! Atlas's own rules still apply to whichever model answers: this sits
//! under the same daemon, persona, claim checks and approval floor. The
//! fallback lifts Apple's over-cautious blocks on ordinary requests; it is
//! not a way around Atlas's own limits.
//!
//! The Swift shell registers a function (`atlas_mobile_apple_model`) that
//! takes the request as JSON and writes the answer as JSON; Android and
//! older iPhones never register one, so they always use Atlas's own model.

use crate::brain::{ChatReply, ChatRequest, Llm, Role};
use crate::error::Result;
use std::ffi::{c_char, CStr, CString};
use std::sync::Mutex;

/// Apple's model's context window, in tokens (FoundationModels, iOS 26).
pub const APPLE_CONTEXT_TOKENS: usize = 4_096;

/// What's left for the answer: a request needs to fit under this.
pub const APPLE_PROMPT_BUDGET: usize = APPLE_CONTEXT_TOKENS - 600;

/// The answer buffer the shell writes into.
pub const ANSWER_BYTES: usize = 64 * 1024;

/// What the shell's function returns.
pub mod code {
    /// Answered: the buffer holds `{"text": "..."}`.
    pub const OK: i32 = 0;
    /// Refused by a guardrail or the model.
    pub const REFUSED: i32 = 1;
    /// Over its context window.
    pub const TOO_LONG: i32 = 2;
    /// Not available on this phone right now (not enabled, not downloaded,
    /// low power, not this kind of phone).
    pub const UNAVAILABLE: i32 = 3;
    /// Anything else went wrong.
    pub const FAILED: i32 = 4;
}

/// The shell's function: request JSON in, answer JSON out (into `out`, at
/// most `out_len` bytes, NUL-terminated), returning one of `code`.
pub type AppleFn = unsafe extern "C" fn(req: *const c_char, out: *mut c_char, out_len: usize) -> i32;

static APPLE: Mutex<Option<AppleFn>> = Mutex::new(None);

/// The Swift shell hands over its function once Apple's model reports
/// itself available, or `None` to take it back (it became unavailable).
///
/// # Safety
/// `f`, when given, must stay callable for the life of the process and be
/// safe to call from any thread.
#[no_mangle]
pub unsafe extern "C" fn atlas_mobile_apple_model(f: Option<AppleFn>) {
    if let Ok(mut g) = APPLE.lock() {
        *g = f;
    }
}

/// Is Apple's model there to ask?
fn registered() -> bool {
    APPLE.lock().map(|g| g.is_some()).unwrap_or(false)
}

/// Why a request went to Atlas's own model rather than Apple's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    NotOnThisPhone,
    TooLong,
    WorldKnowledge,
    NeedsATool,
}

/// What Apple's answer came to.
#[derive(Debug, Clone, PartialEq)]
pub enum Apple {
    Answered(String),
    Refused,
    TooLong,
    Unavailable,
    Failed(String),
}

/// About four characters a token: Apple's tokenizer isn't available here,
/// and over-estimating only sends a long request to Atlas's own model.
fn tokens_in(req: &ChatRequest) -> usize {
    let chars: usize = req.messages.iter().map(|m| m.content.len()).sum::<usize>()
        + req.tools.iter().map(|t| t.to_string().len()).sum::<usize>();
    chars / 4 + 8 * req.messages.len()
}

/// The last thing said, which decides what kind of request this is.
fn last_said(req: &ChatRequest) -> &str {
    req.messages.iter().rev().find(|m| m.role == Role::User).map(|m| m.content.as_str()).unwrap_or("")
}

/// Should this request skip Apple's model? `None`: ask it first.
pub fn skip(req: &ChatRequest, apple_here: bool) -> Option<Skip> {
    if !apple_here {
        return Some(Skip::NotOnThisPhone);
    }
    if tokens_in(req) > APPLE_PROMPT_BUDGET {
        return Some(Skip::TooLong);
    }
    let said = last_said(req);
    if req.force_tool || (!req.tools.is_empty() && crate::doing::looks_like_an_action(said)) {
        return Some(Skip::NeedsATool);
    }
    if needs_world_knowledge(said) {
        return Some(Skip::WorldKnowledge);
    }
    None
}

/// A question about the world, or anything current, that a small on-device
/// model isn't built for (Apple: "not designed to be a chatbot for general
/// world knowledge").
fn needs_world_knowledge(said: &str) -> bool {
    use crate::freshness::Shelf;
    let s = said.to_lowercase();
    if matches!(crate::freshness::shelf_for(&s), Shelf::Volatile | Shelf::Quick) {
        return true;
    }
    const ASKS_ABOUT_THE_WORLD: &[&str] = &[
        "who is ", "who was ", "who's the ", "when did ", "when was ", "what year", "how many people", "capital of",
        "population of", "history of", "how tall is", "how far is", "what happened", "the news", "price of",
        "how much does", "how much is", "worth it", "better than", "compare ",
    ];
    ASKS_ABOUT_THE_WORLD.iter().any(|w| s.contains(w))
}

/// A refusal written as an answer.
fn soft_refusal(text: &str) -> bool {
    let t = text.trim().to_lowercase().replace('\u{2019}', "'");
    const SAYS_NO: &[&str] = &[
        "i can't help with", "i cannot help with", "i can't assist with", "i cannot assist with", "i'm not able to help",
        "i am not able to help", "i'm unable to help", "i can't provide", "i cannot provide", "i'm sorry, but i can't",
        "i'm sorry, but i cannot", "i can't do that", "i won't be able to", "as an ai", "i'm not able to provide",
    ];
    SAYS_NO.iter().any(|p| t.starts_with(p) || t.contains(&format!(". {p}")) || t.contains(&format!("! {p}")))
        || crate::backed::denies_an_ability(&t).is_some()
}

/// The request as the shell reads it: instructions (the system messages),
/// the conversation, and the most to say.
pub fn request_json(req: &ChatRequest) -> String {
    let instructions: Vec<&str> = req.messages.iter().filter(|m| m.role == Role::System).map(|m| m.content.as_str()).collect();
    let turns: Vec<serde_json::Value> = req
        .messages
        .iter()
        .filter(|m| m.role != Role::System)
        .map(|m| serde_json::json!({"role": m.role.name(), "content": m.content}))
        .collect();
    serde_json::json!({
        "instructions": instructions.join("\n\n"),
        "turns": turns,
        "max_tokens": req.max_tokens,
    })
    .to_string()
}

/// What the shell's code and buffer come to.
pub fn read_answer(code: i32, out: &str) -> Apple {
    match code {
        code::OK => match serde_json::from_str::<serde_json::Value>(out) {
            Ok(v) => match v.get("text").and_then(|t| t.as_str()) {
                Some(t) if soft_refusal(t) => Apple::Refused,
                Some(t) if !t.trim().is_empty() => Apple::Answered(t.trim().to_string()),
                _ => Apple::Failed("Apple's model said nothing".into()),
            },
            Err(e) => Apple::Failed(format!("Apple's model's answer wasn't readable: {e}")),
        },
        code::REFUSED => Apple::Refused,
        code::TOO_LONG => Apple::TooLong,
        code::UNAVAILABLE => Apple::Unavailable,
        _ => Apple::Failed(if out.is_empty() { "Apple's model failed".into() } else { out.to_string() }),
    }
}

/// Ask Apple's model through the shell's function.
fn ask_apple(req: &ChatRequest) -> Apple {
    let Some(f) = APPLE.lock().ok().and_then(|g| *g) else { return Apple::Unavailable };
    let Ok(body) = CString::new(request_json(req)) else { return Apple::Failed("the request had a NUL in it".into()) };
    let mut buf = vec![0u8; ANSWER_BYTES];
    let rc = unsafe { f(body.as_ptr(), buf.as_mut_ptr().cast(), buf.len()) };
    let out = unsafe { CStr::from_ptr(buf.as_ptr().cast()) }.to_string_lossy().into_owned();
    read_answer(rc, &out)
}

/// The iPhone's brain: Apple's model first, Atlas's own model for each
/// request it can't do.
pub struct AppleFirst {
    pub own: std::sync::Arc<dyn Llm>,
    /// Who answered the last request, for the log and the trace.
    pub last: Mutex<Option<&'static str>>,
}

impl AppleFirst {
    pub fn new(own: std::sync::Arc<dyn Llm>) -> AppleFirst {
        AppleFirst { own, last: Mutex::new(None) }
    }

    fn answered_by(&self, who: &'static str) {
        if let Ok(mut g) = self.last.lock() {
            *g = Some(who);
        }
    }

    /// Who answered the last request: "apple" or "atlas".
    pub fn last_answered_by(&self) -> Option<&'static str> {
        self.last.lock().ok().and_then(|g| *g)
    }
}

impl Llm for AppleFirst {
    fn complete(&self, system: &str, user: &str) -> Result<String> {
        let req = ChatRequest {
            messages: vec![crate::brain::Msg::system(system), crate::brain::Msg::user(user)],
            max_tokens: 512,
            ..Default::default()
        };
        if skip(&req, registered()).is_none() {
            if let Apple::Answered(t) = ask_apple(&req) {
                self.answered_by("apple");
                return Ok(t);
            }
        }
        self.answered_by("atlas");
        self.own.complete(system, user)
    }

    fn native_chat(&self) -> bool {
        self.own.native_chat()
    }

    fn chat(&self, req: &ChatRequest, on_text: &mut dyn FnMut(&str) -> bool) -> Result<ChatReply> {
        if skip(req, registered()).is_none() {
            match ask_apple(req) {
                Apple::Answered(t) => {
                    self.answered_by("apple");
                    on_text(&t);
                    return Ok(ChatReply { text: t, tool_calls: Vec::new() });
                }
                // Refused, too long after all, gone, or failed: this one
                // request goes to Atlas's own model -- said in the log, so
                // how often Apple's model hands over can be seen.
                other => crate::outln!("phone: Apple's model {}; Atlas's own model answers this one", match other {
                    Apple::Refused => "refused".to_string(),
                    Apple::TooLong => "found it too long".to_string(),
                    Apple::Unavailable => "isn't available right now".to_string(),
                    Apple::Failed(why) => format!("failed ({why})"),
                    Apple::Answered(_) => unreachable!(),
                }),
            }
        }
        self.answered_by("atlas");
        self.own.chat(req, on_text)
    }

    fn has_stronger(&self) -> bool {
        self.own.has_stronger()
    }

    fn complete_hard(&self, system: &str, user: &str) -> Result<String> {
        // Hard work never goes to the small model.
        self.own.complete_hard(system, user)
    }

    fn complete_long(&self, system: &str, user: &str, max_tokens: u32) -> Result<crate::brain::LongReply> {
        self.own.complete_long(system, user, max_tokens)
    }
}
