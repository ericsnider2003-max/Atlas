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
        Power {
            on_battery: false,
            percent: 100,
        }
    }
}

pub struct Throttle {
    pub cfg: PerfConfig,
    current: f32,
    /// Whether the last `next_interval` found something going on.
    busy: bool,
}

impl Throttle {
    pub fn new(cfg: PerfConfig) -> Self {
        let current = cfg.active_tick_secs as f32;
        Throttle {
            cfg,
            current,
            busy: true,
        }
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

        self.busy = busy;
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

    /// Whether the last interval was computed for a busy machine.
    pub fn was_busy(&self) -> bool {
        self.busy
    }

    /// Whether an index rescan is worth its cost right now.
    pub fn may_scan(&self, power: Power, entries: usize) -> bool {
        if power.on_battery && power.percent < self.cfg.battery_floor_pct {
            return false;
        }
        entries < self.cfg.max_index_entries
    }
}

/// How often the friends' door is looked at while it is open (seconds). Its
/// listener is polled without blocking and does not ring the doorbell, so an
/// open door sets a floor on the idle sleep; with no door open there is none.
pub const KIN_DOOR_POLL_SECS: u64 = 5;

/// What the main loop should sleep, in milliseconds (20 Oct 2026).
///
/// The loop used to cap every nap at two seconds "so kin signals and
/// everything polled at the top are never held hostage", which threw away
/// the 60 s idle backoff and the battery multiplier the throttle computes:
/// Atlas woke every 2 s forever and the laptop never reached deep idle. The
/// nap is already cut short by the doorbell (hub request, typed line,
/// microphone, crew news, stop), so the only things a long nap can delay are
/// timers and the friends' door. So: the throttle's interval, shortened to
/// the next known timer, and to the door's poll interval only while a door
/// is open. A busy machine keeps the short active interval, battery or not.
pub fn loop_sleep_ms(
    cfg: &PerfConfig,
    nap_secs: u64,
    busy: bool,
    kin_door_open: bool,
    next_timer_secs: Option<u64>,
) -> u64 {
    let mut secs = nap_secs;
    if busy {
        secs = secs.min(cfg.active_tick_secs.max(1));
    }
    if kin_door_open {
        secs = secs.min(KIN_DOOR_POLL_SECS);
    }
    let mut ms = secs.max(1) * 1000;
    if let Some(t) = next_timer_secs {
        ms = ms.min(t.saturating_mul(1000));
    }
    ms.max(100)
}

/// How long the idle nap may be while something the tick polls, with no
/// due time of its own, is waiting (a brief that may now be given, one held
/// back to retry). Short enough that none is more than this late; long enough
/// that waiting costs next to nothing.
pub const POLLED_ITEM_NAP_SECS: u64 = 15;

/// The soonest the loop must wake for anything the owner will notice, in
/// seconds from now (21 Oct 2026). `loop_sleep_ms` was given only the next
/// scheduler job, so calendar lead-time reminders, a work session's check-in
/// and end, and the brief -- all polled on the tick -- could fire a whole idle
/// interval (60 to 180 s) late. `due_in` are the known next-due times
/// (`None`: that source has nothing coming); `polled_pending` is true while a
/// source with no cheap next-due has something waiting, which caps the nap at
/// `POLLED_ITEM_NAP_SECS`.
pub fn earliest_wake_secs(due_in: &[Option<u64>], polled_pending: bool) -> Option<u64> {
    due_in
        .iter()
        .flatten()
        .copied()
        .chain(polled_pending.then_some(POLLED_ITEM_NAP_SECS))
        .min()
}

#[cfg(test)]
mod pacing_tests {
    use super::*;

    #[test]
    fn every_user_facing_deadline_shortens_an_idle_nap() {
        let cfg = PerfConfig::default();
        // Nothing coming: the idle backoff stands.
        let none = earliest_wake_secs(&[None, None, None], false);
        assert_eq!(none, None);
        assert_eq!(loop_sleep_ms(&cfg, 180, false, false, none), 180_000);
        // A calendar reminder due in 90 s beats a 180 s nap, and a job
        // further off does not lengthen it.
        let t = earliest_wake_secs(&[Some(400), Some(90), None], false);
        assert_eq!(t, Some(90));
        assert_eq!(loop_sleep_ms(&cfg, 180, false, false, t), 90_000);
        // Something polled with no due time caps the nap at 15 s...
        let p = earliest_wake_secs(&[None, None, None], true);
        assert_eq!(loop_sleep_ms(&cfg, 180, false, false, p), 15_000);
        // ...but a nearer known deadline still wins.
        assert_eq!(earliest_wake_secs(&[Some(4)], true), Some(4));
    }

    #[test]
    fn a_work_sessions_check_in_and_end_are_wake_times() {
        let s = crate::worksession::Session::new("writing", 3600, 1_000);
        assert_eq!(s.next_due_in(1_000), 1_800); // halfway
                                                 // Halfway passed and not said (you were away): the tick can't act on
                                                 // it again, so it is not a wake time -- only the end is, never 0
                                                 // forever.
        assert_eq!(s.next_due_in(2_800), 1_800);
        let mut said = s.clone();
        said.checked_in = true;
        assert_eq!(said.next_due_in(1_000), 3_600);
        assert_eq!(said.next_due_in(9_999), 0);
    }

    #[test]
    fn idle_sleeps_a_minute_battery_longer_active_short() {
        let cfg = PerfConfig::default();
        assert_eq!(loop_sleep_ms(&cfg, 60, false, false, None), 60_000);
        assert_eq!(loop_sleep_ms(&cfg, 180, false, false, None), 180_000);
        assert_eq!(loop_sleep_ms(&cfg, 2, true, false, None), 2_000);
        // Busy on battery is not stretched to six seconds.
        assert_eq!(loop_sleep_ms(&cfg, 6, true, false, None), 2_000);
    }

    #[test]
    fn timers_and_an_open_door_shorten_the_sleep() {
        let cfg = PerfConfig::default();
        assert_eq!(loop_sleep_ms(&cfg, 60, false, false, Some(7)), 7_000);
        assert_eq!(loop_sleep_ms(&cfg, 60, false, false, Some(0)), 100);
        assert_eq!(loop_sleep_ms(&cfg, 60, false, true, None), 5_000);
        assert_eq!(loop_sleep_ms(&cfg, 180, false, true, Some(90)), 5_000);
    }

    #[test]
    fn throttle_reports_busy_and_idle() {
        let mut t = Throttle::new(PerfConfig::default());
        let s = Signals {
            idle_secs: 600,
            dwell_secs: 600,
            ..Default::default()
        };
        let mut last = 0;
        for _ in 0..12 {
            last = t.next_interval(&s, Power::default());
        }
        assert!(!t.was_busy());
        assert_eq!(last, 60);
    }
}
