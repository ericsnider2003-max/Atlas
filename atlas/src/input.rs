//! Three ways in, in priority order, with automatic fallback.
//!
//! 1. **Voice** — the primary interface. Wake word, speak, done.
//! 2. **Push-to-talk** — when the wake word stops firing (noisy room, model
//!    struggling), a key press replaces it. Same speech path, manual trigger.
//! 3. **Typing** — when audio itself is broken (no mic, device unplugged,
//!    binaries missing). Always available, never the thing you have to use.
//!
//! Degradation is automatic and announced. You should never be standing there
//! repeating yourself at a machine that quietly stopped listening.

use crate::error::Result;
use crate::doorbell::Sender;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Voice,
    PushToTalk,
    Typed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Utterance {
    pub text: String,
    pub source: Source,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Voice,
    PushToTalk,
    Typed,
}

impl Tier {
    pub fn describe(&self) -> &'static str {
        match self {
            Tier::Voice => "listening for the wake word",
            Tier::PushToTalk => "push-to-talk — wake word isn't working",
            Tier::Typed => "typing only — audio is unavailable",
        }
    }
}

/// Tracks which tier is live and demotes after repeated failures.
pub struct Tiers {
    pub tier: Tier,
    failures: u32,
    /// Consecutive failures before dropping a tier.
    pub patience: u32,
    /// Successes at a degraded tier before trying the better one again.
    pub recover_after: u32,
    successes: u32,
    /// Whether the wake word is switched on. Off, the best tier is
    /// push-to-talk: Atlas neither starts in the wake-word tier nor climbs
    /// back to it (`set_wake`).
    wake_on: bool,
    /// When the last failure counted, and when the wake word was last
    /// dropped (seconds): failures far apart don't add up, and a dropped wake
    /// word is tried again on its own (30 Sep 2026: three hiccups across a
    /// whole day turned it off, and it came back only after five
    /// push-to-talk turns -- in practice, never).
    last_failure_at: u64,
    wake_dropped_at: Option<u64>,
}

/// Failures further apart than this don't add up to dropping a tier.
pub const FAILURES_COUNT_WITHIN_SECS: u64 = 600;
/// How long after dropping the wake word it is tried again by itself.
pub const TRY_WAKE_AGAIN_AFTER_SECS: u64 = 120;

impl Default for Tiers {
    fn default() -> Self {
        Tiers {
            tier: Tier::Voice,
            failures: 0,
            patience: 3,
            recover_after: 5,
            successes: 0,
            wake_on: true,
            last_failure_at: 0,
            wake_dropped_at: None,
        }
    }
}

impl Tiers {
    /// Something went wrong on the current tier. Returns a message to announce
    /// if this caused a demotion.
    pub fn failed(&mut self) -> Option<String> {
        self.failed_at(crate::store::now())
    }

    /// `failed`, at a given moment.
    pub fn failed_at(&mut self, now: u64) -> Option<String> {
        if now.saturating_sub(self.last_failure_at) > FAILURES_COUNT_WITHIN_SECS {
            self.failures = 0;
        }
        self.last_failure_at = now;
        self.failures += 1;
        self.successes = 0;
        if self.failures < self.patience {
            return None;
        }
        self.failures = 0;
        let next = match self.tier {
            Tier::Voice => Tier::PushToTalk,
            Tier::PushToTalk => Tier::Typed,
            Tier::Typed => return None, // nowhere lower to go
        };
        if next == Tier::PushToTalk {
            self.wake_dropped_at = Some(now);
        }
        self.tier = next;
        Some(format!("Switching to {}.", next.describe()))
    }

    /// The wake word switched on or off (27 Sep 2026). Off moves the voice
    /// tier down to push-to-talk, so the loop stops recording a clip and
    /// transcribing it every pass for a word it was told not to listen for;
    /// on lets it climb back. Typing-only (no audio) is left alone.
    pub fn set_wake(&mut self, on: bool) {
        self.wake_on = on;
        if !on && self.tier == Tier::Voice {
            self.tier = Tier::PushToTalk;
            self.failures = 0;
        } else if on && self.tier == Tier::PushToTalk {
            self.tier = Tier::Voice;
            self.failures = 0;
        }
    }

    pub fn wake_on(&self) -> bool {
        self.wake_on
    }

    /// A turn worked. Enough of these and Atlas tries the better tier again,
    /// because a temporarily noisy room shouldn't permanently demote you.
    pub fn succeeded(&mut self) -> Option<String> {
        self.failures = 0;
        // Already at the best tier there is: the wake word, or push-to-talk
        // when the wake word is switched off.
        if self.tier == Tier::Voice || (!self.wake_on && self.tier == Tier::PushToTalk) {
            return None;
        }
        self.successes += 1;
        if self.successes < self.recover_after {
            return None;
        }
        self.successes = 0;
        let next = match self.tier {
            Tier::Typed => Tier::PushToTalk,
            _ => Tier::Voice,
        };
        self.tier = next;
        Some(format!("Back to {}.", next.describe()))
    }

    /// The microphone just gave words. At typing-only that settles it: audio
    /// isn't unavailable, so Atlas is back to push-to-talk at once rather
    /// than after five more tries -- which it could never get, because
    /// typing-only doesn't listen (29 Sep 2026).
    pub fn heard_you(&mut self) -> Option<String> {
        if self.tier != Tier::Typed {
            return self.succeeded();
        }
        self.tier = Tier::PushToTalk;
        self.failures = 0;
        self.successes = 0;
        Some(format!("Back to {}.", Tier::PushToTalk.describe()))
    }

    /// Push-to-talk because the wake word's microphone failed, and it works
    /// again: straight back to the wake word, rather than after five turns
    /// at push-to-talk that nobody takes when the wake word is what they use
    /// (29 Sep 2026). Nothing when the wake word is off or already in use.
    pub fn microphone_works_again(&mut self) -> Option<String> {
        if self.tier != Tier::PushToTalk || !self.wake_on {
            return None;
        }
        self.tier = Tier::Voice;
        self.failures = 0;
        self.successes = 0;
        Some(format!("Back to {}.", Tier::Voice.describe()))
    }

    /// Audio is unusable outright — skip straight to typing.
    /// The wake word, tried again by itself a while after failures dropped
    /// it -- a microphone busy for a moment, a speech engine that crashed
    /// once, recover without you pressing a key five times. `Some` when it
    /// is back on.
    pub fn try_the_wake_word_again(&mut self, now: u64) -> Option<String> {
        let dropped = self.wake_dropped_at?;
        if self.tier != Tier::PushToTalk || !self.wake_on || now.saturating_sub(dropped) < TRY_WAKE_AGAIN_AFTER_SECS {
            return None;
        }
        self.wake_dropped_at = None;
        self.failures = 0;
        self.successes = 0;
        self.tier = Tier::Voice;
        Some(format!("Back to {}.", Tier::Voice.describe()))
    }

    pub fn audio_unavailable(&mut self) -> Option<String> {
        if self.tier == Tier::Typed {
            return None;
        }
        self.tier = Tier::Typed;
        self.failures = 0;
        Some(format!("Switching to {}.", Tier::Typed.describe()))
    }
}

/// Hold-to-talk on a key you also need for typing.
///
/// Tab is the configured key, and Tab is a key you press hundreds of times a
/// day. Grabbing it globally would break tabbing everywhere. So Atlas watches
/// how long it is held: a normal tap passes straight through to whatever you
/// were doing, and only a deliberate hold starts recording. The key is only
/// swallowed once the hold threshold is crossed, which means a tap is never
/// delayed or eaten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyEvent {
    /// Not a hold. Let the app have it.
    PassThrough,
    /// Threshold crossed — start recording, and swallow the key.
    StartTalking,
    /// Released after a hold. Stop recording and send what was captured.
    StopTalking,
    /// Still deciding.
    Waiting,
}

#[derive(Debug)]
pub struct HoldToTalk {
    pub key: String,
    /// How long the key must be down before it counts as push-to-talk.
    pub hold_ms: u64,
    down_at: Option<u64>,
    talking: bool,
}

impl HoldToTalk {
    pub fn new(key: &str, hold_ms: u64) -> Self {
        HoldToTalk { key: key.to_string(), hold_ms, down_at: None, talking: false }
    }

    pub fn down(&mut self, now_ms: u64) -> KeyEvent {
        if self.down_at.is_none() {
            self.down_at = Some(now_ms);
        }
        self.poll(now_ms)
    }

    /// Called while the key stays down, so the hold can trigger before release
    /// — you should hear it start listening, not find out afterwards.
    pub fn poll(&mut self, now_ms: u64) -> KeyEvent {
        match self.down_at {
            Some(t0) if !self.talking && now_ms.saturating_sub(t0) >= self.hold_ms => {
                self.talking = true;
                KeyEvent::StartTalking
            }
            Some(_) => KeyEvent::Waiting,
            None => KeyEvent::PassThrough,
        }
    }

    pub fn up(&mut self, now_ms: u64) -> KeyEvent {
        let held = self.down_at.map(|t0| now_ms.saturating_sub(t0)).unwrap_or(0);
        self.down_at = None;
        if self.talking {
            self.talking = false;
            return KeyEvent::StopTalking;
        }
        let _ = held;
        KeyEvent::PassThrough
    }

    pub fn is_talking(&self) -> bool {
        self.talking
    }
}

/// Typed input runs on its own thread and lands in the same queue as speech,
/// so you can always type even while Atlas is listening.
pub struct Keyboard {
    rx: Receiver<Utterance>,
    _tx: Sender<Utterance>,
}

impl Keyboard {
    pub fn spawn() -> Keyboard {
        let (tx, rx) = crate::doorbell::channel();
        let tx2 = tx.clone();
        std::thread::spawn(move || {
            use std::io::BufRead;
            let stdin = std::io::stdin();
            for line in stdin.lock().lines().map_while(|l| l.ok()) {
                let t = line.trim().to_string();
                if t.is_empty() {
                    continue;
                }
                if tx2.send(Utterance { text: t, source: Source::Typed }).is_err() {
                    break;
                }
            }
        });
        Keyboard { rx, _tx: tx }
    }

    /// A way in for typed words from somewhere other than this console --
    /// the typing box (H1) -- landing in the same queue.
    pub fn sender(&self) -> Sender<Utterance> {
        self._tx.clone()
    }

    /// Anything typed while Atlas was busy. Never blocks.
    pub fn poll(&self) -> Option<Utterance> {
        self.rx.try_recv().ok()
    }

    pub fn wait(&self, ms: u64) -> Option<Utterance> {
        match self.rx.recv_timeout(Duration::from_millis(ms)) {
            Ok(u) => Some(u),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => None,
        }
    }
}

/// Can Atlas speak here, whatever the microphone is doing? Voice switched on
/// and the speech tool present (29 Sep 2026: typing-only, which a broken
/// microphone causes, also silenced every reply -- one broken part turning
/// off another that worked).
pub fn can_speak(tools: Option<&crate::voice::ToolsConfig>) -> bool {
    tools.is_some_and(|t| t.enabled && t.tts.available(&t.vars))
}

/// Check the voice stack is usable before relying on it, so Atlas can drop to
/// typing at startup rather than after three silent failures.
pub fn audio_available(tools: Option<&crate::voice::ToolsConfig>) -> Result<bool> {
    let Some(t) = tools else { return Ok(false) };
    if !t.enabled {
        return Ok(false);
    }
    let vars = t.vars.clone();
    Ok(t.record.available(&vars) && t.stt.available(&vars) && t.tts.available(&vars))
}
