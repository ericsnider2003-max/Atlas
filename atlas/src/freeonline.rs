//! Free models online, with no account and no key: Atlas's second choice.
//!
//! Eric, 30 Sep 2026: "we are still offline first online second", and an
//! online model is fine "if I don't have to set up an account and it's
//! free". These are the services that answered from his laptop that day with
//! nothing but a request -- no sign-up, no key:
//!
//! - **Kilo's gateway** (`kilo-auto/free`): about 1.4 s for a sentence.
//!   Its free pool may pass requests to providers that keep them.
//! - **Pollinations** (`openai`, which was gpt-oss-20b): about 7-8 s.
//! - **OVHcloud AI Endpoints**, anonymous: two requests a minute per address
//!   per model; every model refused with 429 that evening, so it is last.
//!
//! None of them promises to stay free or keep nothing, so what leaves is
//! scrubbed first (`redact`: keys, card and account numbers, email addresses
//! and phone numbers become placeholders and are put back in the answer),
//! each service that refuses is rested before it's asked again, and the
//! setting `models.online_second` turns all of it off.
//!
//! Used only when the model on this machine can't answer (`FallbackLlm`), or
//! when there isn't one.

use crate::brain::{dig, Llm};
use crate::error::{AtlasError, Result};

/// One free service.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Provider {
    pub name: &'static str,
    /// Its OpenAI-shaped chat address.
    pub url: &'static str,
    pub model: &'static str,
}

/// In the order they are asked.
pub const PROVIDERS: &[Provider] = &[
    Provider { name: "Kilo", url: "https://api.kilo.ai/api/gateway/chat/completions", model: "kilo-auto/free" },
    Provider { name: "Pollinations", url: "https://text.pollinations.ai/openai", model: "openai" },
    Provider { name: "OVHcloud", url: "https://oai.endpoints.kepler.ai.cloud.ovh.net/v1/chat/completions", model: "gpt-oss-20b" },
];

/// How long a service that said "too many requests" is left alone, seconds.
pub const REST_AFTER_REFUSAL_SECS: u64 = 120;
/// And one that failed any other way.
pub const REST_AFTER_FAILURE_SECS: u64 = 600;

/// The request body.
fn body(p: &Provider, system: &str, user: &str, max_tokens: u32) -> String {
    serde_json::json!({
        "model": p.model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
        "max_tokens": max_tokens,
        "temperature": 0.7,
        "stream": false,
    })
    .to_string()
}

/// What a service answered: the reply's words, or why there aren't any.
/// `Err((refused, why))`: `refused` is a rate limit, which rests the service
/// for less time than anything else.
pub fn reply_from(raw: &str) -> std::result::Result<String, (bool, String)> {
    let v: serde_json::Value = match serde_json::from_str(raw.trim()) {
        Ok(v) => v,
        Err(_) => {
            let low = raw.to_lowercase();
            let refused = low.contains("429") || low.contains("too many") || low.contains("rate limit");
            let short: String = raw.chars().take(160).collect();
            return Err((refused, format!("not an answer: {short}")));
        }
    };
    if let Some(text) = dig(&v, "choices.0.message.content").and_then(|t| t.as_str()) {
        let text = crate::phonemodel::without_thinking(text).trim().to_string();
        if !text.is_empty() {
            return Ok(text);
        }
        return Err((false, "an empty answer".into()));
    }
    let why = dig(&v, "error.message")
        .or_else(|| dig(&v, "message"))
        .or_else(|| dig(&v, "error"))
        .map(|m| m.as_str().map(str::to_string).unwrap_or_else(|| m.to_string()))
        .unwrap_or_else(|| "no answer in the reply".into());
    let low = why.to_lowercase();
    Err((low.contains("rate limit") || low.contains("too many") || low.contains("429"), why))
}

/// The free services, asked in turn.
pub struct FreeOnline {
    providers: Vec<Provider>,
    /// Which services are resting, and until when (seconds).
    resting: std::sync::Mutex<Vec<(&'static str, u64)>>,
    /// Sends one body to one address, returns what came back. `curl` in
    /// Atlas; a stand-in in tests.
    send: Box<dyn Fn(&str, &str) -> Result<String> + Send + Sync>,
    /// Which service answered last, for "where did that answer come from".
    pub last_answered_by: std::sync::Mutex<Option<&'static str>>,
}

impl FreeOnline {
    /// Through `curl`, which every supported Windows has.
    pub fn new() -> FreeOnline {
        FreeOnline::with_sender(PROVIDERS.to_vec(), Box::new(send_with_curl))
    }

    pub fn with_sender(providers: Vec<Provider>, send: Box<dyn Fn(&str, &str) -> Result<String> + Send + Sync>) -> FreeOnline {
        FreeOnline {
            providers,
            resting: std::sync::Mutex::new(Vec::new()),
            send,
            last_answered_by: std::sync::Mutex::new(None),
        }
    }

    fn rest(&self, name: &'static str, secs: u64) {
        let until = crate::store::now() + secs;
        if let Ok(mut r) = self.resting.lock().or_else(crate::crash::unpoison) {
            r.retain(|(n, _)| *n != name);
            r.push((name, until));
        }
    }

    fn is_resting(&self, name: &str) -> bool {
        let now = crate::store::now();
        self.resting.lock().or_else(crate::crash::unpoison).map(|r| r.iter().any(|(n, until)| *n == name && *until > now)).unwrap_or(false)
    }

    /// Ask each service that isn't resting, in order, until one answers.
    pub fn ask(&self, system: &str, user: &str) -> Result<String> {
        self.ask_for(system, user, 700).map(|(text, _)| text)
    }

    /// `ask`, with room for `max_tokens`, and whether the service said it
    /// ran out of room (`finish_reason: "length"`) -- what the code builder
    /// needs (2 Oct 2026: 700 was a sentence's worth, not a file's).
    fn ask_for(&self, system: &str, user: &str, max_tokens: u32) -> Result<(String, bool)> {
        // The phone app sends nothing to these until you've said yes
        // (`phonemode`): the question goes back to you instead.
        if crate::phonemode::on() && !crate::phonemode::online_ok() {
            return Err(crate::error::AtlasError::Platform(crate::phonemode::ASK_ONLINE.into()));
        }
        if crate::brain::google_data_held() {
            return Err(crate::error::AtlasError::Platform(crate::brain::GOOGLE_STAYS_HERE.into()));
        }
        // Scrubbed here as well as in `FallbackLlm`: with no model on this
        // machine this is the only model, and nothing else scrubs for it.
        let mut scrub = crate::redact::Scrubber::default();
        let (mut system, user) = (scrub.scrub(system), scrub.scrub(user));
        if let Some(note) = scrub.say() {
            system.push_str(&format!(
                "\n\n({note}; they appear as placeholders like ⟦EMAIL_1⟧. Use the placeholders exactly as written.)"
            ));
        }
        let mut why: Vec<String> = Vec::new();
        for p in &self.providers {
            if self.is_resting(p.name) {
                continue;
            }
            match (self.send)(p.url, &body(p, &system, &user, max_tokens)).map(|raw| (reply_from(&raw), cut_off_in(&raw))) {
                Ok((Ok(text), cut)) => {
                    if let Ok(mut l) = self.last_answered_by.lock().or_else(crate::crash::unpoison) {
                        *l = Some(p.name);
                    }
                    return Ok((scrub.put_back(&text), cut));
                }
                Ok((Err((refused, w)), _)) => {
                    self.rest(p.name, if refused { REST_AFTER_REFUSAL_SECS } else { REST_AFTER_FAILURE_SECS });
                    why.push(format!("{}: {w}", p.name));
                }
                Err(e) => {
                    self.rest(p.name, REST_AFTER_FAILURE_SECS);
                    why.push(format!("{}: {e}", p.name));
                }
            }
        }
        Err(AtlasError::Platform(if why.is_empty() {
            "the free online models are all resting after refusing lately".into()
        } else {
            format!("no free online model answered ({})", why.join("; "))
        }))
    }
}

impl Default for FreeOnline {
    fn default() -> Self {
        FreeOnline::new()
    }
}

impl Llm for FreeOnline {
    fn complete(&self, system: &str, user: &str) -> Result<String> {
        self.ask(system, user)
    }

    fn complete_long(&self, system: &str, user: &str, max_tokens: u32) -> Result<crate::brain::LongReply> {
        self.ask_for(system, user, max_tokens).map(|(text, cut_off)| crate::brain::LongReply { text, cut_off })
    }

    /// What these services take in at least; each says more of its own.
    fn context_tokens(&self) -> Option<u32> {
        Some(32_000)
    }
}

/// Did a service's raw reply say it stopped for want of room?
fn cut_off_in(raw: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(raw.trim()).map(|v| crate::brain::says_cut_off(&v)).unwrap_or(false)
}

/// One POST through `curl`: the body on stdin, three minutes at most (a
/// whole file of code takes longer than a sentence; 2 Oct 2026).
fn send_with_curl(url: &str, body: &str) -> Result<String> {
    let tool = crate::tools::ExternalTool {
        command: "curl".into(),
        args: ["-s", "-S", "-m", "180", "-X", "POST", "-H", "Content-Type: application/json", "--data-binary", "@-", url]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        stdin_text: true,
        result_file: None,
        timeout_secs: 190,
    };
    tool.run(&Default::default(), Some(body))
}
