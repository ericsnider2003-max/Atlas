//! One doorbell for the whole of Atlas: whatever has news for the main loop
//! rings it, and anything idle waits on it instead of waking on a timer.
//!
//! ## Why (5 Oct 2026 audit, Q14)
//!
//! Idle, Atlas woke about 20 times a second in `nap_awake` (a 10 ms keyboard
//! wait, then a 40 ms hub wait, round and round), and once a tick in
//! `goodbye::nap`'s 50 ms slices. Each wakeup is cheap, but together they keep
//! a laptop's processor out of its deep idle states, which is battery.
//! Polling was used because the news arrives on three separate channels
//! (typed lines, the microphone, the hub) and a thread can only block on one.
//!
//! The doorbell is the one thing to block on. A sender made by [`channel`]
//! rings it after every send, so a wait returns the moment there is news,
//! and an idle Atlas sleeps until something happens (or its deadline).
//!
//! ## The one rule that makes it race-free
//!
//! Read [`rung`] *before* looking at the channels, then [`wait_after`] that
//! count. News that arrives between the look and the wait has already moved
//! the count, so the wait returns at once rather than sleeping through it.

use std::sync::{Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

static RUNG: Mutex<u64> = Mutex::new(0);
static BELL: Condvar = Condvar::new();

/// Something has news: wake everything waiting.
pub fn ring() {
    let mut n = RUNG.lock().unwrap_or_else(PoisonError::into_inner);
    *n = n.wrapping_add(1);
    BELL.notify_all();
}

/// How many times the bell has rung. Read it before looking for news.
pub fn rung() -> u64 {
    *RUNG.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The longest one wait sleeps before looking again. On Unix a stop can come
/// from a signal handler, which may only set a flag (`goodbye::mark_stop`),
/// not ring; so a wait there wakes five times a second to look. Windows'
/// console handler runs on an ordinary thread and rings, so a wait there
/// sleeps as long as it was asked to.
pub const LONGEST_SLEEP_MS: Option<u64> = if cfg!(unix) { Some(200) } else { None };

/// Wait until the bell rings again after `seen` (a count from [`rung`]), or
/// `ms` passes (at most `LONGEST_SLEEP_MS`). Returns at once if it already
/// has. True when it rang.
pub fn wait_after(seen: u64, ms: u64) -> bool {
    // Capped only where there is a cap (not on Windows); and a very long
    // wait can't overflow the clock: past what `Instant` can hold, it waits a
    // day and looks again.
    let ms = LONGEST_SLEEP_MS.map_or(ms, |most| ms.min(most));
    let now = Instant::now();
    let until = now.checked_add(Duration::from_millis(ms)).unwrap_or(now + Duration::from_secs(86_400));
    let mut n = RUNG.lock().unwrap_or_else(PoisonError::into_inner);
    while *n == seen {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return false;
        }
        n = BELL.wait_timeout(n, left).unwrap_or_else(PoisonError::into_inner).0;
    }
    true
}

/// An `mpsc` sender that rings the doorbell after each send.
pub struct Sender<T>(std::sync::mpsc::Sender<T>);

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Sender(self.0.clone())
    }
}

impl<T> Sender<T> {
    pub fn send(&self, t: T) -> Result<(), std::sync::mpsc::SendError<T>> {
        let r = self.0.send(t);
        ring();
        r
    }
}

/// `std::sync::mpsc::channel`, with a sender that rings.
pub fn channel<T>() -> (Sender<T>, std::sync::mpsc::Receiver<T>) {
    let (tx, rx) = std::sync::mpsc::channel();
    (Sender(tx), rx)
}
