//! Staying out of the way.
//!
//! An always-on assistant that costs you 5% CPU forever is a tax you pay every
//! second of every day. The design rule here is that Atlas does nothing on a
//! schedule that it could do on an event, and when nothing is happening it
//! backs off geometrically until it is checking in about once a minute.

use crate::awareness::Signals;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PerfConfig {
    /// Poll interval when you are actively working, in seconds.
    pub active_tick_secs: u64,
    /// Ceiling once nothing has happened for a while.
    pub idle_tick_secs: u64,
    /// Backoff multiplier per quiet tick.
    pub backoff: f32,
    /// Multiply every interval by this on battery.
    pub battery_multiplier: f32,
    /// Skip index rescans entirely below this battery percentage.
    pub battery_floor_pct: u8,
    /// Hard ceiling on index entries. Beyond this, narrow your roots.
    pub max_index_entries: usize,
}

impl Default for PerfConfig {
    fn default() -> Self {
        PerfConfig {
            active_tick_secs: 2,
            idle_tick_secs: 60,
            backoff: 1.6,
            battery_multiplier: 3.0,
            battery_floor_pct: 20,
            max_index_entries: 200_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Power {
    pub on_battery: bool,
    pub percent: u8,
}

impl Default for Power {
    fn default() -> Self {
        Power { on_battery: false, percent: 100 }
    }
}

pub struct Throttle {
    pub cfg: PerfConfig,
    current: f32,
}

impl Throttle {
    pub fn new(cfg: PerfConfig) -> Self {
        let current = cfg.active_tick_secs as f32;
        Throttle { cfg, current }
    }

    /// How long to sleep before the next tick.
    ///
    /// Something happened → snap back to the fast interval immediately.
    /// Nothing happened → back off. Responsiveness matters at exactly the
    /// moment you start doing something, and not at all before then.
    pub fn next_interval(&mut self, s: &Signals, power: Power) -> u64 {
        let busy = s.in_conversation
            || !s.recent_changes.is_empty()
            || s.dwell_secs < 5
            || s.idle_secs < 30;

        if busy {
            self.current = self.cfg.active_tick_secs as f32;
        } else {
            self.current = (self.current * self.cfg.backoff).min(self.cfg.idle_tick_secs as f32);
        }

        let mut secs = self.current;
        if power.on_battery {
            secs *= self.cfg.battery_multiplier;
        }
        secs.round().max(1.0) as u64
    }

    /// Whether an index rescan is worth its cost right now.
    pub fn may_scan(&self, power: Power, entries: usize) -> bool {
        if power.on_battery && power.percent < self.cfg.battery_floor_pct {
            return false;
        }
        entries < self.cfg.max_index_entries
    }
}
