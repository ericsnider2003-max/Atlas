//! When the graphics reset under the language model (12 Oct 2026).
//!
//! On Eric's laptop (integrated Intel graphics, Vulkan build of llama.cpp,
//! every layer offloaded, 8192 context, picture encoder on the graphics) the
//! model server's own log read `vk::Queue::submit: ErrorDeviceLost` /
//! `ggml_vulkan: device lost on Vulkan0` while reading a prompt of 1,000-2,000
//! tokens. That is the driver resetting the device (Windows' timeout
//! detection) after a graphics job ran too long -- and the server *does not
//! exit*: it keeps listening, `/health` keeps saying `ok`, and every request
//! after it fails. So Atlas trusted a running process, restarted nothing, and
//! each turn waited on a server that could not answer.
//!
//! This module is the memory and the decisions around that, kept apart from
//! the daemon so they can be tested without a server:
//!
//! * recognising the reset in the server's words (`device_lost_in`);
//! * a record, kept in the store, of how often and since when
//!   (`Trouble`) -- the next starts read it, so they do not crash the same
//!   way again;
//! * the safe settings a start uses while that record stands (`apply`):
//!   no layers on the graphics, no work offloaded to it, the picture encoder
//!   on the processor, a smaller context;
//! * the `atlas doctor` line (`finding`);
//! * a flag the model connection checks, so a turn asked while the server is
//!   known to be unable fails at once and the rules-only path answers,
//!   instead of waiting out a read timeout.

use crate::models::ModelsConfig;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// The store record.
pub const RECORD: &str = "gpu_trouble";

/// How long safe mode stands before the graphics are tried once more: two
/// weeks. A driver update can fix it; a reset is then a single failed start.
pub const SAFE_FOR_SECS: u64 = 14 * 24 * 60 * 60;

/// The context a safe start asks for, in tokens: half the usual 8192, still
/// room for the prompts seen on the laptop (up to about 2,000 tokens).
pub const SAFE_CONTEXT: u64 = 4096;

/// What is said to a caller while the server cannot answer.
pub const UNAVAILABLE: &str = "the model server is recovering from a graphics reset";

/// What Atlas remembers of the graphics resetting.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Trouble {
    /// How many resets have been seen, ever.
    pub resets: u32,
    pub first_at: u64,
    pub last_at: u64,
    /// The model that was running at the last reset.
    pub model: String,
    /// The server's own line about it.
    pub detail: String,
    /// When safe mode began; 0: not in it.
    pub safe_since: u64,
}

/// Does the server's text say the graphics device was lost?
pub fn device_lost_in(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    t.contains("errordevicelost") || t.contains("device lost on") || t.contains("device_lost")
}

/// The line that says so, for the record.
pub fn lost_line_in(text: &str) -> Option<String> {
    let line = text.lines().rev().find(|l| device_lost_in(l))?;
    let mut line = line.trim().to_string();
    if line.len() > 200 {
        let mut cut = 200;
        while !line.is_char_boundary(cut) {
            cut -= 1;
        }
        line.truncate(cut);
    }
    Some(line)
}

/// The end of the model server's log, as text: the last 64 KB, so a long
/// session's log is not read whole on every look.
pub fn log_tail() -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let path = crate::roots::data_dir()
        .join("logs")
        .join("model-server.log");
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let from = len.saturating_sub(64 * 1024);
    f.seek(SeekFrom::Start(from)).ok()?;
    let mut bytes = Vec::new();
    f.take(64 * 1024).read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Whether safe mode stands at `now`.
pub fn safe_now(t: &Trouble, now: u64) -> bool {
    t.safe_since != 0 && now < t.safe_since.saturating_add(SAFE_FOR_SECS)
}

/// Note one more reset: safe mode begins (or carries on, if a start in safe
/// mode somehow lost its device too -- then it is not renewed, so the
/// two-week clock cannot be pushed out for ever by a fault that is not the
/// graphics').
pub fn record_reset(t: &mut Trouble, model: &str, detail: &str, now: u64) {
    if t.first_at == 0 {
        t.first_at = now;
    }
    t.resets = t.resets.saturating_add(1);
    t.last_at = now;
    t.model = model.to_string();
    t.detail = detail.to_string();
    if !safe_now(t, now) {
        t.safe_since = now;
    }
}

/// The settings a start uses: `cfg` as it is, or -- while safe mode stands
/// and you haven't said `gpu_layers: off` already -- the safe version.
pub fn apply(mut cfg: ModelsConfig, t: &Trouble, now: u64) -> ModelsConfig {
    if !safe_now(t, now) {
        return cfg;
    }
    cfg.gpu_safe = true;
    cfg.gpu_layers = "off".into();
    cfg.context = match cfg.context {
        0 => SAFE_CONTEXT,
        c => c.min(SAFE_CONTEXT),
    };
    cfg
}

/// The `atlas doctor` line: `(ok, detail)`. `None` when the graphics have
/// never reset. Not "ok" while safe mode stands: the model is running slower
/// than it should, and that is a finding.
pub fn finding(t: &Trouble, now: u64) -> Option<(bool, String)> {
    if t.resets == 0 {
        return None;
    }
    let when = |secs: u64| {
        let (y, m, d) = crate::market::time::civil_from_days((secs / 86_400) as i64);
        format!("{y:04}-{m:02}-{d:02}")
    };
    let times = if t.resets == 1 {
        "once".to_string()
    } else {
        format!("{} times", t.resets)
    };
    if safe_now(t, now) {
        let again = when(t.safe_since.saturating_add(SAFE_FOR_SECS));
        Some((
            false,
            format!(
                "GPU kept resetting ({times}; last {}); the model runs on the processor in safe \
                 mode since {} -- slower, but it stays up. The graphics are tried again on {again}, \
                 or now with `atlas doctor --retry-gpu`.",
                when(t.last_at),
                when(t.safe_since)
            ),
        ))
    } else {
        Some((
            true,
            format!(
                "the graphics reset {times} (last {}) but safe mode has ended; the model is back \
                 on the graphics",
                when(t.last_at)
            ),
        ))
    }
}

pub fn load() -> Trouble {
    crate::roots::store().load(RECORD)
}

pub fn save(t: &Trouble) -> crate::error::Result<()> {
    crate::roots::store().save(RECORD, t)
}

/// `atlas doctor --retry-gpu`: forget safe mode, so the next start uses the
/// graphics again. The count of resets is kept.
pub fn retry_graphics() -> crate::error::Result<()> {
    let mut t = load();
    t.safe_since = 0;
    save(&t)
}

// ---- the live flag -------------------------------------------------------

/// When the server was last found to have lost its graphics, in seconds
/// (0: not flagged).
static LOST_AT: AtomicU64 = AtomicU64::new(0);

/// How long the flag holds without being cleared or renewed: a safe start
/// loads in about a minute, and a process with no daemon to restart anything
/// (a one-off ask) must not refuse questions for ever.
pub const FLAG_SECS: u64 = 150;

/// The server is known to have lost its graphics and has not been replaced.
pub fn flag() {
    flag_at(crate::store::now());
}

pub fn flag_at(now: u64) {
    LOST_AT.store(now.max(1), Ordering::SeqCst);
}

/// A server that answers has taken its place.
pub fn clear() {
    LOST_AT.store(0, Ordering::SeqCst);
}

pub fn flagged() -> bool {
    flagged_at(crate::store::now())
}

pub fn flagged_at(now: u64) -> bool {
    let at = LOST_AT.load(Ordering::SeqCst);
    at != 0 && now.saturating_sub(at) < FLAG_SECS
}

/// The error a model call gets while the flag stands.
pub fn unavailable() -> crate::error::AtlasError {
    crate::error::AtlasError::Platform(UNAVAILABLE.into())
}

static SAFE_RUNNING: AtomicBool = AtomicBool::new(false);

/// The server last started was started in safe mode (`apply`): the context
/// the rest of Atlas plans prompts for is the smaller one.
pub fn set_safe_running(on: bool) {
    SAFE_RUNNING.store(on, Ordering::SeqCst);
}

pub fn safe_running() -> bool {
    SAFE_RUNNING.load(Ordering::SeqCst)
}
