//! Muse Spark, Meta's model, for whoever connects their own account
//! (6 Oct 2026).
//!
//! People on Eric's wait list asked for it. What it is, checked that day:
//! closed (no weights to run here), served at `api.meta.ai` behind an
//! OpenAI-shaped API, a million-token window, reads pictures, PDFs and video.
//! The API has no free tier and **no spending cap of any kind** -- it bills a
//! card for whatever is sent. And there is no "sign in with Meta" that bills
//! the person signing in, so each person brings their own key from
//! dev.meta.ai. Eric pays for nobody: Atlas never ships a key of its own.
//!
//! So Atlas adds the cap Meta lacks: a monthly limit in dollars, counted from
//! what each answer says it used, after which Muse is not asked again until
//! the month turns, and Atlas says so.
//!
//! Where it is used once connected: the hard work (drafts, research
//! write-ups, long answers -- `complete_hard` / `complete_long`), "ask Muse
//! ..." said outright, and "think hard about ..." at its deepest effort.
//! Talking stays on the model on this machine: free, and quick to answer.
//! What goes out is scrubbed first, like everything else that leaves.

use crate::brain::{ChatReply, ChatRequest, LongReply, Llm};
use crate::error::{AtlasError, Result};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

pub const HOST: &str = "api.meta.ai";
pub const PATH: &str = "/v1/chat/completions";
/// Where a key is made: the Model API dashboard, "API keys".
pub const KEY_PAGE: &str = "https://dev.meta.ai/";
/// The vault entry the key is kept under.
pub const VAULT_MUSE: &str = "muse spark key";

/// The settings (`models.muse`).
#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct MuseConfig {
    /// The model asked for.
    pub model: String,
    /// The most a month may cost, in US dollars. 0: no limit (not
    /// recommended -- Meta has none either).
    pub monthly_cap_usd: f64,
    /// Meta's prices, per million tokens, in and out. Kept here so a price
    /// change is a setting, not a new Atlas.
    pub price_in_per_million: f64,
    pub price_out_per_million: f64,
    /// How hard it thinks on ordinary hard work: minimal, low, medium, high,
    /// xhigh. "think hard about" always asks for high.
    pub effort: String,
}

impl Default for MuseConfig {
    fn default() -> Self {
        MuseConfig {
            model: "muse-spark-1.3".into(),
            monthly_cap_usd: 5.0,
            // Muse Spark 1.1/1.2 list prices (Meta Model API, Jul-Sep 2026).
            price_in_per_million: 1.25,
            price_out_per_million: 4.25,
            effort: "medium".into(),
        }
    }
}

/// The efforts Meta takes. "none" is refused by the API.
pub const EFFORTS: &[&str] = &["minimal", "low", "medium", "high", "xhigh"];

// ---------------------------------------------------------------- the key

static KEY: Mutex<Option<String>> = Mutex::new(None);

/// Set by the daemon once the vault is open, and cleared on Disconnect.
pub fn set_key(key: Option<String>) {
    if let Ok(mut k) = KEY.lock().or_else(crate::crash::unpoison) {
        *k = key.filter(|k| !k.trim().is_empty());
    }
}

pub fn has_key() -> bool {
    KEY.lock().or_else(crate::crash::unpoison).map(|k| k.is_some()).unwrap_or(false)
}

fn key() -> Option<String> {
    KEY.lock().or_else(crate::crash::unpoison).ok().and_then(|k| k.clone())
}

// ---------------------------------------------------------------- the money

/// This month's spending, in millionths of a dollar, and which month that is
/// (days since 1970 of the month's first day, so "is it still this month"
/// is a comparison of numbers).
static SPENT_MICRO: AtomicU64 = AtomicU64::new(0);
static MONTH: AtomicU64 = AtomicU64::new(0);
static CAP_MICRO: AtomicU64 = AtomicU64::new(5_000_000);

/// The month `now` falls in, as "YYYY-MM".
fn month_of(now: u64) -> String {
    let (y, m, _) = crate::hubpages::ymd((now / 86_400) as i64);
    format!("{y:04}-{m:02}")
}

fn month_number(now: u64) -> u64 {
    let (y, m, _) = crate::hubpages::ymd((now / 86_400) as i64);
    (y as u64) * 12 + m as u64
}

/// What's been spent, kept by the daemon across restarts.
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct Ledger {
    pub month: String,
    pub spent_usd: f64,
    pub calls: u64,
}

pub const LEDGER: &str = "muse_spend";

impl Ledger {
    /// The larger of two counts for the same month: setting a new limit
    /// never forgets what was already spent.
    pub fn max_of(&self, other: &Ledger) -> Ledger {
        if self.month != other.month || self.spent_usd >= other.spent_usd {
            self.clone()
        } else {
            other.clone()
        }
    }
}

static CALLS: AtomicU64 = AtomicU64::new(0);

/// Take the stored ledger and the cap in (the daemon, at start and when the
/// settings change).
pub fn restore(l: &Ledger, cap_usd: f64, now: u64) {
    CAP_MICRO.store(to_micro(cap_usd), Ordering::SeqCst);
    MONTH.store(month_number(now), Ordering::SeqCst);
    if l.month == month_of(now) {
        SPENT_MICRO.store(to_micro(l.spent_usd), Ordering::SeqCst);
        CALLS.store(l.calls, Ordering::SeqCst);
    } else {
        SPENT_MICRO.store(0, Ordering::SeqCst);
        CALLS.store(0, Ordering::SeqCst);
    }
}

/// The ledger as it stands, for the daemon to keep.
pub fn ledger(now: u64) -> Ledger {
    roll_month(now);
    Ledger { month: month_of(now), spent_usd: SPENT_MICRO.load(Ordering::SeqCst) as f64 / 1e6, calls: CALLS.load(Ordering::SeqCst) }
}

fn to_micro(usd: f64) -> u64 {
    if usd.is_finite() && usd > 0.0 {
        (usd * 1e6).round() as u64
    } else {
        0
    }
}

fn roll_month(now: u64) {
    let m = month_number(now);
    if MONTH.swap(m, Ordering::SeqCst) != m {
        SPENT_MICRO.store(0, Ordering::SeqCst);
        CALLS.store(0, Ordering::SeqCst);
    }
}

/// Has this month's limit been reached? A cap of 0 is no limit.
pub fn over_cap(now: u64) -> bool {
    roll_month(now);
    let cap = CAP_MICRO.load(Ordering::SeqCst);
    cap > 0 && SPENT_MICRO.load(Ordering::SeqCst) >= cap
}

/// Connected and under the month's limit: worth asking.
pub fn ready(now: u64) -> bool {
    has_key() && !over_cap(now)
}

/// What one answer cost, from the token counts it reports.
fn cost_usd(prompt_tokens: u64, completion_tokens: u64, cfg: &MuseConfig) -> f64 {
    prompt_tokens as f64 / 1e6 * cfg.price_in_per_million + completion_tokens as f64 / 1e6 * cfg.price_out_per_million
}

fn charge(usd: f64, now: u64) {
    roll_month(now);
    SPENT_MICRO.fetch_add(to_micro(usd), Ordering::SeqCst);
    CALLS.fetch_add(1, Ordering::SeqCst);
}

/// "$0.42 of $5.00 this month, 31 answers."
pub fn spent_sentence(now: u64) -> String {
    let l = ledger(now);
    let cap = CAP_MICRO.load(Ordering::SeqCst) as f64 / 1e6;
    let limit = if cap > 0.0 { format!(" of your ${cap:.2} limit") } else { " (no limit set)".to_string() };
    format!("${:.2}{limit} this month, {} answer{}", l.spent_usd, l.calls, if l.calls == 1 { "" } else { "s" })
}

// ---------------------------------------------------------------- asking

/// The request body: an OpenAI-shaped chat, with Meta's effort setting.
/// Meta refuses `stop`, `logprobs`, `n` and an effort of "none", so none of
/// them is ever sent.
pub fn muse_request(cfg: &MuseConfig, system: &str, user: &str, max_tokens: u32, effort: &str) -> String {
    let effort = if EFFORTS.contains(&effort) { effort } else { "medium" };
    serde_json::json!({
        "model": cfg.model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
        "max_completion_tokens": max_tokens,
        "reasoning_effort": effort,
    })
    .to_string()
}

/// The answer's words, whether it ran out of room, and the tokens it says it
/// used (in, out).
pub fn answer_of(raw: &str) -> std::result::Result<(String, bool, u64, u64), String> {
    let v: serde_json::Value = serde_json::from_str(raw.trim()).map_err(|_| {
        let short: String = raw.chars().take(160).collect();
        format!("not an answer: {short}")
    })?;
    let used = |k: &str| crate::brain::dig(&v, &format!("usage.{k}")).and_then(|x| x.as_u64()).unwrap_or(0);
    let (tin, tout) = (used("prompt_tokens"), used("completion_tokens"));
    if let Some(text) = crate::brain::dig(&v, "choices.0.message.content").and_then(|t| t.as_str()) {
        let text = crate::phonemodel::without_thinking(text).trim().to_string();
        if text.is_empty() {
            return Err("an empty answer".into());
        }
        return Ok((text, crate::brain::says_cut_off(&v), tin, tout));
    }
    let why = crate::brain::dig(&v, "error.message")
        .and_then(|m| m.as_str().map(str::to_string))
        .unwrap_or_else(|| "no answer in the reply".into());
    Err(why)
}

/// Sends one body with a key and gives back the raw reply. Real HTTPS in
/// Atlas; a stand-in in tests.
pub type Sender = Box<dyn Fn(&str, &str) -> std::result::Result<String, String> + Send + Sync>;

fn send_for_real(key: &str, body: &str) -> std::result::Result<String, String> {
    let auth = format!("Bearer {key}");
    let (r, _) = crate::http::https_call(
        "POST",
        HOST,
        PATH,
        &[("Authorization", auth.as_str())],
        Some(("application/json", body)),
        std::time::Duration::from_secs(180),
    )
    .map_err(|e| e.to_string())?;
    match r.status {
        200 => Ok(r.body),
        401 | 403 => Err("Meta refused the key -- connect Muse again with a new one".into()),
        402 => Err("Meta says the account needs a payment method or more credit".into()),
        429 => Err("Meta asked to slow down; try again in a minute".into()),
        s => Err(format!("Meta answered {s}: {}", r.body.chars().take(160).collect::<String>())),
    }
}

/// Does Meta take this key? Asks for the list of models, which costs
/// nothing.
pub fn check_key(key: &str) -> std::result::Result<(), String> {
    let auth = format!("Bearer {key}");
    let (r, _) = crate::http::https_call("GET", HOST, "/v1/models", &[("Authorization", auth.as_str())], None, std::time::Duration::from_secs(20))
        .map_err(|e| format!("couldn't reach Meta: {e}"))?;
    match r.status {
        200 => Ok(()),
        401 | 403 => Err("Meta says the key isn't valid".into()),
        s => Err(format!("Meta answered {s}")),
    }
}

/// Muse Spark, as a model Atlas can ask.
pub struct MuseSpark {
    pub cfg: MuseConfig,
    send: Sender,
}

impl MuseSpark {
    pub fn new(cfg: MuseConfig) -> MuseSpark {
        MuseSpark { cfg, send: Box::new(send_for_real) }
    }

    pub fn with_sender(cfg: MuseConfig, send: Sender) -> MuseSpark {
        MuseSpark { cfg, send }
    }

    /// One question, scrubbed on the way out and put back on the way in,
    /// counted against the month's limit.
    pub fn ask(&self, system: &str, user: &str, max_tokens: u32, effort: &str) -> Result<LongReply> {
        let now = crate::store::now();
        if crate::phonemode::on() && !crate::phonemode::online_ok() {
            return Err(AtlasError::Platform(crate::phonemode::ASK_ONLINE.into()));
        }
        let Some(key) = key() else {
            return Err(AtlasError::Platform("Muse isn't connected (Accounts page, Muse Spark)".into()));
        };
        if over_cap(now) {
            return Err(AtlasError::Platform(format!("Muse has reached this month's limit ({}); it starts again next month, or raise the limit on the Accounts page", spent_sentence(now))));
        }
        let mut scrub = crate::redact::Scrubber::default();
        let (mut system, user) = (scrub.scrub(system), scrub.scrub(user));
        if let Some(note) = scrub.say() {
            system.push_str(&format!("\n\n({note}; they appear as placeholders like ⟦EMAIL_1⟧. Use the placeholders exactly as written.)"));
        }
        let raw = (self.send)(&key, &muse_request(&self.cfg, &system, &user, max_tokens, effort)).map_err(AtlasError::Platform)?;
        let (text, cut_off, tin, tout) = answer_of(&raw).map_err(|e| AtlasError::Platform(format!("Muse: {e}")))?;
        charge(cost_usd(tin, tout, &self.cfg), now);
        Ok(LongReply { text: scrub.put_back(&text), cut_off })
    }
}

impl Llm for MuseSpark {
    fn complete(&self, system: &str, user: &str) -> Result<String> {
        let effort = self.cfg.effort.clone();
        self.ask(system, user, 4_000, &effort).map(|r| r.text)
    }

    fn complete_long(&self, system: &str, user: &str, max_tokens: u32) -> Result<LongReply> {
        let effort = self.cfg.effort.clone();
        self.ask(system, user, max_tokens, &effort)
    }

    /// Far more than this, but every token is billed: no prompt is let grow
    /// past what a local model would take just because it could.
    fn context_tokens(&self) -> Option<u32> {
        Some(128_000)
    }
}

/// The models Atlas already has, with Muse asked first for the hard work
/// while it's connected and under its limit. Checked at each call, so
/// connecting or disconnecting takes effect without a restart.
pub struct MuseFirst {
    pub muse: std::sync::Arc<MuseSpark>,
    pub rest: std::sync::Arc<dyn Llm>,
}

impl Llm for MuseFirst {
    fn complete(&self, system: &str, user: &str) -> Result<String> {
        self.rest.complete(system, user)
    }

    fn native_chat(&self) -> bool {
        self.rest.native_chat()
    }

    fn chat(&self, req: &ChatRequest, on_text: &mut dyn FnMut(&str) -> bool) -> Result<ChatReply> {
        self.rest.chat(req, on_text)
    }

    fn chat_until(&self, req: &ChatRequest, on_text: &mut dyn FnMut(&str) -> bool, keep_going: &dyn Fn() -> bool) -> Result<ChatReply> {
        self.rest.chat_until(req, on_text, keep_going)
    }

    fn has_stronger(&self) -> bool {
        ready(crate::store::now()) || self.rest.has_stronger()
    }

    fn complete_hard(&self, system: &str, user: &str) -> Result<String> {
        // The hard work carries the conversation, which can carry what Atlas
        // read from Google: not to Muse while that's held.
        if ready(crate::store::now()) && !crate::brain::google_data_held() {
            if let Ok(t) = self.muse.complete(system, user) {
                return Ok(t);
            }
        }
        self.rest.complete_hard(system, user)
    }

    fn complete_long(&self, system: &str, user: &str, max_tokens: u32) -> Result<LongReply> {
        // The hard work carries the conversation, which can carry what Atlas
        // read from Google: not to Muse while that's held.
        if ready(crate::store::now()) && !crate::brain::google_data_held() {
            if let Ok(r) = self.muse.complete_long(system, user, max_tokens) {
                return Ok(r);
            }
        }
        self.rest.complete_long(system, user, max_tokens)
    }

    fn context_tokens(&self) -> Option<u32> {
        self.rest.context_tokens()
    }
}

/// "ask Muse what ...", "muse, ...": the question, when it is one for Muse.
pub fn asked_of_muse(said: &str) -> Option<String> {
    let t = said.trim();
    let low = t.to_ascii_lowercase();
    for p in ["ask muse spark ", "ask muse ", "muse spark, ", "muse, ", "hey muse "] {
        if low.starts_with(p) {
            let q = t[p.len()..].trim().trim_start_matches([',', ':']).trim();
            let q = q.strip_prefix("to ").unwrap_or(q);
            return (!q.is_empty()).then(|| q.to_string());
        }
    }
    None
}
