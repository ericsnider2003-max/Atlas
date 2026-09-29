//! Two guards for anything that talks to something outside Atlas: a rate
//! limit, and a breaker that stops calling a thing that is down.
//!
//! **Sources:** GCRA (the Generic Cell Rate Algorithm) as `boinkor-net/governor`
//! (MIT) implements it — one number of state per key, the theoretical arrival
//! time: with `t` the interval per cell and `τ = t·(burst−1)`, a request at
//! `now` is refused if `now < TAT − τ`, else `TAT = max(TAT, now) + t`. The
//! breaker is the Closed → Open → HalfOpen machine of `dmexe/failsafe-rs`
//! (MIT). Clean-room.
//!
//! **Why Atlas wants it.**
//! * The **online-secondary model** (`FallbackLlm`'s secondary slot, and the
//!   coming WireGuard server): when it is down, every request today waits out
//!   a timeout before falling back. A breaker learns it is down after a few
//!   failures and falls straight back to local until a probe says otherwise.
//! * **Outbound** — outreach, SMTP, the Telegram channel — needs a ceiling so
//!   a bug or a loop cannot send three hundred messages in a minute. GCRA gives
//!   that with one integer per key and no background timer.
//! * **Anything that calls another Atlas** has the same shape in its degrade
//!   path. A breaker makes "unreachable" a state with a reason instead of a
//!   per-call timeout.
//!
//! All times are passed in (ms). Nothing here reads a clock, so it is exact
//! under test and replay.

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Gcra {
    /// Milliseconds per request at the sustained rate.
    pub interval_ms: u64,
    /// Requests allowed back-to-back from rest.
    pub burst: u64,
    tat: HashMap<String, u64>,
}

impl Gcra {
    /// `per` requests every `period_ms`, bursting up to `burst`.
    pub fn new(per: u64, period_ms: u64, burst: u64) -> Gcra {
        assert!(per > 0 && burst > 0);
        Gcra { interval_ms: (period_ms / per).max(1), burst, tat: HashMap::new() }
    }

    /// `Ok(())` to go ahead, or `Err(ms)` to wait that long first.
    pub fn check(&mut self, key: &str, now: u64) -> Result<(), u64> {
        let t = self.interval_ms;
        let tau = t * (self.burst - 1);
        let tat = *self.tat.get(key).unwrap_or(&now);
        let earliest = tat.saturating_sub(tau);
        if now < earliest {
            return Err(earliest - now);
        }
        self.tat.insert(key.to_string(), tat.max(now) + t);
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Closed,
    /// Calls refused until this time (ms).
    Open { until: u64 },
    /// One trial call allowed.
    HalfOpen,
}

#[derive(Debug, Clone)]
pub struct Breaker {
    pub failures_to_open: u32,
    /// First open period; doubles on every failed probe, up to `max_open_ms`.
    pub open_ms: u64,
    pub max_open_ms: u64,
    state: State,
    consecutive: u32,
    current_open: u64,
    trial_out: bool,
}

impl Breaker {
    pub fn new(failures_to_open: u32, open_ms: u64, max_open_ms: u64) -> Breaker {
        Breaker {
            failures_to_open: failures_to_open.max(1),
            open_ms,
            max_open_ms,
            state: State::Closed,
            consecutive: 0,
            current_open: open_ms,
            trial_out: false,
        }
    }

    pub fn state(&self, now: u64) -> State {
        match self.state {
            State::Open { until } if now >= until => State::HalfOpen,
            s => s,
        }
    }

    /// May a call go out now? In HalfOpen exactly one trial is let through.
    pub fn allow(&mut self, now: u64) -> bool {
        match self.state(now) {
            State::Closed => true,
            State::Open { .. } => false,
            State::HalfOpen => {
                self.state = State::HalfOpen;
                if self.trial_out {
                    false
                } else {
                    self.trial_out = true;
                    true
                }
            }
        }
    }

    pub fn success(&mut self) {
        self.state = State::Closed;
        self.consecutive = 0;
        self.current_open = self.open_ms;
        self.trial_out = false;
    }

    pub fn failure(&mut self, now: u64) {
        match self.state(now) {
            State::HalfOpen => {
                self.current_open = (self.current_open * 2).min(self.max_open_ms);
                self.state = State::Open { until: now + self.current_open };
                self.trial_out = false;
            }
            _ => {
                self.consecutive += 1;
                if self.consecutive >= self.failures_to_open {
                    self.state = State::Open { until: now + self.current_open };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gcra_bursts_then_holds_the_rate() {
        // 10 per minute, burst 3
        let mut g = Gcra::new(10, 60_000, 3);
        assert!(g.check("smtp", 0).is_ok());
        assert!(g.check("smtp", 0).is_ok());
        assert!(g.check("smtp", 0).is_ok());
        let wait = g.check("smtp", 0).unwrap_err();
        assert_eq!(wait, 6_000);
        assert!(g.check("smtp", 6_000).is_ok());
        assert!(g.check("smtp", 6_000).is_err());
        // keys are independent
        assert!(g.check("telegram", 0).is_ok());
    }

    #[test]
    fn gcra_sustained_rate_over_an_hour() {
        let mut g = Gcra::new(10, 60_000, 3);
        let mut sent = 0;
        let mut now = 0;
        while now < 3_600_000 {
            if g.check("k", now).is_ok() {
                sent += 1;
            }
            now += 100;
        }
        // 600 sustained + at most burst-1 extra
        assert!((600..=602).contains(&sent), "{sent}");
    }

    #[test]
    fn breaker_opens_probes_backs_off_and_closes() {
        let mut b = Breaker::new(3, 1_000, 8_000);
        for t in 0..3 {
            assert!(b.allow(t));
            b.failure(t);
        }
        assert!(!b.allow(10));
        assert!(matches!(b.state(10), State::Open { .. }));
        // after 1s: one trial only
        assert!(b.allow(1_010));
        assert!(!b.allow(1_011));
        b.failure(1_020);
        // backed off to 2s
        assert!(!b.allow(2_500));
        assert!(b.allow(3_030));
        b.success();
        assert_eq!(b.state(3_031), State::Closed);
        assert!(b.allow(3_032));
    }

    #[test]
    fn backoff_is_capped() {
        let mut b = Breaker::new(1, 1_000, 4_000);
        b.failure(0);
        let mut now = 0;
        for _ in 0..10 {
            now += 10_000;
            assert!(b.allow(now));
            b.failure(now);
        }
        assert_eq!(b.state(now), State::Open { until: now + 4_000 });
    }
}
