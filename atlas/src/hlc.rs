//! The clock two devices order their histories by, when their wall clocks
//! disagree.
//!
//! Atlas merges by replaying both devices' event logs in order (see `sync`).
//! "In order" needs a timestamp both sides compute the same way. Wall-clock
//! time alone can't be that timestamp: your phone and your laptop are never
//! perfectly in step, and a phone that has been offline in a drawer can come
//! back a few seconds behind. Order by raw wall time and an edit you made on
//! the phone *after* one on the laptop can sort *before* it, so the merge
//! settles on the wrong answer — silently, and only when the clocks happen to
//! be skewed, which is the worst kind of bug to find later.
//!
//! A **Hybrid Logical Clock** fixes exactly that. It's the well-worn design
//! from Kulkarni et al. (2014) — the same one CockroachDB and most local-first
//! systems use — kept in-house here because it's a page of logic, not a
//! dependency. The robustness details (the drift guard below, the carry on
//! counter overflow) are adapted from Eclipse Zenoh's `uhlc-rs`, the most
//! battle-tested HLC in Rust, translated to Atlas's seconds-and-"nothing is
//! lost" world rather than pulled in as a crate.
//!
//! A stamp carries two numbers:
//!
//! - `wall`: physical time (Atlas uses seconds, `store::now()`), never allowed
//!   to go backwards even if the OS clock does.
//! - `count`: a tie-break that steps up when several events land in the same
//!   `wall` second, or when a message arrives stamped in the same second.
//!
//! Three promises: a device's own stamps only move forward; a stamp read off an
//! incoming event pulls this device's clock up to meet it, so anything it does
//! next sorts *after* everything it just learned; and `wall` stays within a
//! bound of true time — see the drift guard — so the ordering still reads like
//! real time to a person, and one device with a wildly wrong clock can't drag
//! everyone else's into the next decade.
//!
//! Total order across devices is `(wall, count, device-id)`: this module gives
//! the first two, and `sync` breaks the last tie by device id so both sides
//! reach the identical order without talking to each other.

use serde::{Deserialize, Serialize};

/// How far ahead of this device's own physical clock an incoming stamp is
/// allowed to pull it. Beyond this, the stamp is treated as a wrong clock: the
/// clock is advanced only to the cap (so future local stamps still read like
/// now), and the caller is told how far off the other device looks.
///
/// `uhlc` defaults to 500ms for machines expected to be NTP-synced. Atlas runs
/// on personal devices — a phone that has been off, a laptop whose clock nobody
/// checks — and its time is in seconds, so the bound is an hour: generous
/// enough that ordinary skew and time-zone confusion never trip it, tight
/// enough that a device stuck in the wrong year can't poison the order.
pub const MAX_AHEAD_SECS: u64 = 3_600;

/// One reading of the clock: physical time plus a same-instant tie-break.
///
/// Ordered by `wall` then `count`. Two stamps from different devices can still
/// be equal here; the caller settles that by device id, which is stable and
/// unique. `Default` is the zero stamp, which sorts before every real one —
/// that's deliberate, so an event from before this clock existed (a bundle
/// written by an older Atlas, whose events carry no stamp) reads as "long ago"
/// rather than jumping to the front.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Stamp {
    /// Physical time in seconds, monotonic on this device.
    pub wall: u64,
    /// Tie-break within one `wall` second.
    pub count: u32,
}

impl Stamp {
    pub const ZERO: Stamp = Stamp { wall: 0, count: 0 };

    /// Has this event ever been stamped, or is it a bare pre-HLC one?
    pub fn is_set(&self) -> bool {
        self.wall != 0 || self.count != 0
    }

    /// The next stamp strictly after this one that stays in the same second
    /// when it can, and carries into the next second when the counter is full.
    /// The single place same-second stepping and overflow are handled, so no
    /// caller has to think about the 1-in-4-billion carry.
    fn stepped(self) -> Stamp {
        match self.count.checked_add(1) {
            Some(count) => Stamp { wall: self.wall, count },
            // Four billion events in one second is unreachable in practice;
            // handled anyway because "can't happen" is how clocks break.
            None => Stamp { wall: self.wall.saturating_add(1), count: 0 },
        }
    }
}

impl PartialOrd for Stamp {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Stamp {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.wall.cmp(&other.wall).then(self.count.cmp(&other.count))
    }
}

impl std::fmt::Display for Stamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.wall, self.count)
    }
}

/// Another device's clock looks wrong: its stamp was this many seconds ahead of
/// ours. Not an error to abort on — the event is kept and ordered by its own
/// stamp, which is the "nothing is lost" rule — but worth saying out loud, so
/// the person can fix the clock rather than wonder why one device's notes keep
/// sorting to the far future.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Skew {
    pub ahead_secs: u64,
}

impl Skew {
    /// In the words Atlas would say it.
    pub fn plain(&self, device: &str) -> String {
        let d = self.ahead_secs / 86_400;
        let h = (self.ahead_secs % 86_400) / 3_600;
        let how = if d > 0 {
            format!("{d} day{}", if d == 1 { "" } else { "s" })
        } else if h > 0 {
            format!("{h} hour{}", if h == 1 { "" } else { "s" })
        } else {
            format!("{} minutes", self.ahead_secs.max(60) / 60)
        };
        format!(
            "{device}'s clock is about {how} ahead of this one. I've taken its notes in and \
             ordered them by their own time — nothing's lost — but you may want to fix its clock, \
             or they'll keep sorting to the future."
        )
    }
}

/// What a receive produced: the stamp for the receive, and whether the other
/// device's clock looked wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recv {
    pub stamp: Stamp,
    pub skew: Option<Skew>,
}

/// A device's clock. Keep one per install and let it ride the sync log.
///
/// It is just the last stamp handed out. Persist it (it serialises) so a
/// restart doesn't hand out a stamp the device has already used — reuse is the
/// one thing that breaks the "own stamps only move forward" promise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Clock {
    last: Stamp,
}

impl Clock {
    pub fn new() -> Clock {
        Clock { last: Stamp::ZERO }
    }

    /// The last stamp this clock produced, read without advancing it. The
    /// clock persists between runs through serde on the whole `Clock` (it rides
    /// on `sync::Log`), so nothing in Atlas needs this to save or restore —
    /// it's the read accessor the HLC's own tests assert ordering through, and
    /// is listed in `dead_methods::TEST_ONLY_METHODS` for exactly that reason.
    pub fn peek(&self) -> Stamp {
        self.last
    }

    /// A fresh stamp for something happening on *this* device now.
    ///
    /// `physical` is the wall clock in seconds (`store::now()`). If it hasn't
    /// advanced past the last stamp — same second, or an OS clock that slipped
    /// backwards — the counter carries the order instead, and `wall` never
    /// retreats.
    pub fn tick(&mut self, physical: u64) -> Stamp {
        let stamp = if physical > self.last.wall {
            Stamp { wall: physical, count: 0 }
        } else {
            // Same second, or the OS clock went backwards: hold `wall` at the
            // high-water mark and let the counter carry the order.
            self.last.stepped()
        };
        self.last = stamp;
        stamp
    }

    /// Fold in a stamp that arrived on an event from another device, and return
    /// a stamp for *receiving* it — after both this clock and the incoming one.
    /// After this, anything this device does sorts after the message it just
    /// took in, which is the causal guarantee the merge needs.
    ///
    /// The drift guard: an incoming `wall` more than [`MAX_AHEAD_SECS`] past our
    /// physical time is capped before it touches our clock, and reported as
    /// [`Skew`]. The event itself keeps its own stamp elsewhere; this only
    /// stops a wrong remote clock from dragging ours forward.
    pub fn receive(&mut self, incoming: Stamp, physical: u64) -> Recv {
        let cap = physical.saturating_add(MAX_AHEAD_SECS);
        let (adopt, skew) = if incoming.wall > cap {
            (
                Stamp { wall: cap, count: 0 },
                Some(Skew { ahead_secs: incoming.wall.saturating_sub(physical) }),
            )
        } else {
            (incoming, None)
        };

        let last = self.last;
        let wall = physical.max(last.wall).max(adopt.wall);
        let stamp = if wall == last.wall && wall == adopt.wall {
            // Same second as both: step past the higher of the two counters.
            Stamp { wall, count: last.count.max(adopt.count) }.stepped()
        } else if wall == last.wall {
            last.stepped()
        } else if wall == adopt.wall {
            adopt.stepped()
        } else {
            // Physical time is strictly beyond both: a clean new second.
            Stamp { wall, count: 0 }
        };
        self.last = stamp;
        Recv { stamp, skew }
    }

    /// Fold in the newest stamp from a batch of incoming events without minting
    /// a receive event of our own — just make sure the clock is past it, so
    /// future local events sort after all of them. Returns [`Skew`] if that
    /// device's clock looked wrong. A no-op when we're already past `seen`.
    pub fn observe(&mut self, seen: Stamp, physical: u64) -> Option<Skew> {
        if seen <= self.last {
            return None;
        }
        self.receive(seen, physical).skew
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_stamps_strictly_increase_even_within_one_second() {
        let mut c = Clock::new();
        let a = c.tick(100);
        let b = c.tick(100);
        let d = c.tick(100);
        assert!(a < b && b < d, "same-second ticks must still order: {a} {b} {d}");
        assert_eq!(a.wall, 100);
        assert_eq!((a.count, b.count, d.count), (0, 1, 2));
    }

    #[test]
    fn wall_never_goes_backwards_when_the_os_clock_slips() {
        let mut c = Clock::new();
        let a = c.tick(1_000);
        let b = c.tick(990);
        assert!(b > a, "a backwards OS clock must not produce a smaller stamp");
        assert_eq!(b.wall, 1_000, "wall holds at the high-water mark");
        assert_eq!(b.count, 1);
    }

    #[test]
    fn receiving_pulls_the_clock_up_so_later_local_work_sorts_after() {
        let mut phone = Clock::new();
        phone.tick(50);
        let from_laptop = Stamp { wall: 70, count: 3 };
        let recv = phone.receive(from_laptop, 50);
        assert!(recv.stamp > from_laptop, "receive sorts after the incoming event");
        assert!(recv.skew.is_none(), "20 seconds is ordinary skew, not a wrong clock");
        let next = phone.tick(50);
        assert!(next > from_laptop, "local work after a receive sorts after it");
        assert!(next > recv.stamp);
    }

    #[test]
    fn two_skewed_devices_agree_on_causal_order() {
        let mut laptop = Clock::new();
        let mut phone = Clock::new();
        let laptop_edit = laptop.tick(2_000);
        phone.receive(laptop_edit, 1_992); // phone 8s behind
        let phone_edit = phone.tick(1_992);
        assert!(
            phone_edit > laptop_edit,
            "the phone edit happened after and must sort after, despite a behind clock: \
             laptop={laptop_edit} phone={phone_edit}"
        );
    }

    #[test]
    fn a_wildly_wrong_remote_clock_is_capped_and_reported_not_adopted() {
        // A device whose clock is set ten years ahead. Ours must not chase it.
        let mut c = Clock::new();
        c.tick(1_000);
        let from_the_future = Stamp { wall: 1_000 + 10 * 365 * 86_400, count: 0 };
        let recv = c.receive(from_the_future, 1_000);
        let skew = recv.skew.expect("a decade ahead must be flagged as skew");
        assert!(skew.ahead_secs > 9 * 365 * 86_400);
        // Our clock advanced only to the cap, not to the year 2036.
        assert!(
            c.peek().wall <= 1_000 + MAX_AHEAD_SECS,
            "the clock must not be dragged past the cap: {}",
            c.peek()
        );
        assert!(skew.plain("the spare laptop").contains("ordered them by their own time"));
    }

    #[test]
    fn ordinary_skew_inside_the_bound_is_not_flagged() {
        let mut c = Clock::new();
        c.tick(1_000);
        // Half an hour ahead — under the hour bound, so adopted silently.
        let recv = c.receive(Stamp { wall: 1_000 + 1_800, count: 0 }, 1_000);
        assert!(recv.skew.is_none());
        assert_eq!(c.peek().wall, 1_000 + 1_800, "an in-bound stamp is adopted");
    }

    #[test]
    fn the_counter_carries_into_the_next_second_when_it_overflows() {
        // A clock loaded from disk with its counter already at the ceiling —
        // constructed the way production actually restores one, by deserializing
        // the `Clock` (it rides on `sync::Log`), not through a test-only
        // constructor. So this proves the real restart path carries correctly.
        let mut c: Clock =
            serde_json::from_str(r#"{"last":{"wall":42,"count":4294967295}}"#).unwrap();
        let next = c.tick(42); // same second, counter already full
        assert_eq!(next, Stamp { wall: 43, count: 0 }, "a full counter carries the second");
        assert!(next > Stamp { wall: 42, count: u32::MAX });
    }

    #[test]
    fn observe_advances_without_minting_an_event_and_reports_skew() {
        let mut c = Clock::new();
        c.tick(10);
        assert!(c.observe(Stamp { wall: 500, count: 9 }, 10).is_none());
        let next = c.tick(10);
        assert!(next > Stamp { wall: 500, count: 9 }, "after observing, next is past it");
        // Observing a stamp we're already past is a no-op.
        assert!(c.observe(Stamp { wall: 1, count: 0 }, 10).is_none());
    }

    #[test]
    fn a_bare_pre_hlc_stamp_sorts_before_any_real_one() {
        assert!(Stamp::ZERO < Stamp { wall: 1, count: 0 });
        assert!(!Stamp::ZERO.is_set());
        assert!(Stamp { wall: 1, count: 0 }.is_set());
    }

    #[test]
    fn resuming_from_disk_never_reuses_a_stamp() {
        // The whole safety of a restart: a device that comes back must not mint
        // a stamp it already used before it went down. Atlas persists the clock
        // by serializing it with the log, so this restores it the same way —
        // serde in, then tick — rather than through a synthetic constructor.
        let used = Stamp { wall: 300, count: 5 };
        let saved = serde_json::to_string(&Clock { last: used }).unwrap();
        let mut c: Clock = serde_json::from_str(&saved).unwrap();
        assert!(c.tick(300) > used, "a restart must not hand out a stamp at or before the last used");
    }
}
