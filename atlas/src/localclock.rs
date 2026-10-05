//! The time on this machine's own clock, for anything shown to you.
//!
//! Atlas stores every moment as UTC seconds, which is right: a UTC second
//! means the same thing on the laptop, the phone and the server. But a time
//! *shown* to you has to be the one on your wall. The hub's command deck
//! (Eric's design, 23 Sep 2026) puts times on the day's timeline and against
//! everything Atlas did, and "14:02" read in UTC is four hours out for
//! someone in New York.
//!
//! The offset is asked of the operating system rather than configured, the
//! same reasoning as `daemon::local_offset_mins`: a config field would be a
//! second declaration of a fact Windows already knows, and it would be wrong
//! twice a year. That function asks `date +%z`, which doesn't exist on
//! Windows; this one asks Windows directly.

use std::sync::Mutex;

/// Seconds to add to UTC to get this machine's wall clock. Asked of the
/// system at most once a minute: the hub renders often, and Atlas runs for
/// weeks, so the night the clocks change is picked up within a minute
/// rather than at the next restart.
pub fn offset_secs() -> i64 {
    if let Some(told) = told_offset() {
        return told;
    }
    // Your `time_zone` setting, when you've chosen one: the same zone the
    // calendar and every "at 7" use (`tz`), so the hub and the calendar can
    // never disagree about what time it is. Merged 26 Sep 2026: this module
    // (the third chat) read only the machine; `tz` (round 3) read only the
    // setting and fell back to UTC. Now one home zone: the setting if set,
    // otherwise this machine's clock.
    if let Some(z) = home_zone() {
        return z.offset_at(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0));
    }
    machine_offset_secs()
}

static HOME: Mutex<Option<crate::tz::Zone>> = Mutex::new(None);

/// Set once at start, and again whenever you change the `time_zone` setting:
/// `None` means "this machine's clock".
pub fn set_home_zone(z: Option<crate::tz::Zone>) {
    *HOME.lock().unwrap_or_else(|p| p.into_inner()) = z;
}

/// The zone times are shown in now: the one you chose, or this machine's.
pub fn zone() -> crate::tz::Zone {
    match told_offset() {
        Some(off) => crate::tz::Zone::fixed(off),
        None => home_zone().unwrap_or_else(crate::tz::machine),
    }
}

fn home_zone() -> Option<crate::tz::Zone> {
    HOME.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

/// The machine's own offset, ignoring any zone you've chosen:
/// `ATLAS_CLOCK_OFFSET` if set, otherwise what the operating system says.
pub fn machine_offset_secs() -> i64 {
    if let Some(told) = told_offset() {
        return told;
    }
    static CACHE: Mutex<Option<(std::time::Instant, i64)>> = Mutex::new(None);
    let mut c = CACHE.lock().unwrap_or_else(|p| p.into_inner());
    match *c {
        Some((at, v)) if at.elapsed() < std::time::Duration::from_secs(60) => v,
        _ => {
            let v = read_offset();
            *c = Some((std::time::Instant::now(), v));
            v
        }
    }
}

/// `ATLAS_CLOCK_OFFSET`, in seconds east of UTC, when it is set: for a machine
/// whose zone is set wrong and can't be changed, and for the test suite,
/// which pins it to 0 (`.cargo/config.toml`) so a test written at noon UTC
/// means noon wherever the tests happen to run.
pub fn told_offset() -> Option<i64> {
    if let Some(p) = *PINNED.lock().unwrap_or_else(|p| p.into_inner()) {
        return Some(p);
    }
    std::env::var("ATLAS_CLOCK_OFFSET").ok()?.trim().parse().ok()
}

static PINNED: Mutex<Option<i64>> = Mutex::new(None);

/// Pin this process's clock to `offset` seconds east of UTC, as
/// `ATLAS_CLOCK_OFFSET` does, or unpin it with `None`.
///
/// For the tests that are about a time of day (28 Sep 2026): they passed only
/// when cargo found `.cargo/config.toml` -- run from the crate's folder --
/// and failed on any other machine's zone when run from anywhere else, or as
/// the built test program on its own. They pin it themselves now.
pub fn pin_offset(offset: Option<i64>) {
    *PINNED.lock().unwrap_or_else(|p| p.into_inner()) = offset;
}

#[cfg(windows)]
fn read_offset() -> i64 {
    #[repr(C)]
    struct SystemTime {
        _f: [u16; 8],
    }
    #[repr(C)]
    struct TimeZoneInformation {
        bias: i32,
        standard_name: [u16; 32],
        standard_date: SystemTime,
        standard_bias: i32,
        daylight_name: [u16; 32],
        daylight_date: SystemTime,
        daylight_bias: i32,
    }
    extern "system" {
        fn GetTimeZoneInformation(tz: *mut TimeZoneInformation) -> u32;
    }
    let mut tz = TimeZoneInformation {
        bias: 0,
        standard_name: [0; 32],
        standard_date: SystemTime { _f: [0; 8] },
        standard_bias: 0,
        daylight_name: [0; 32],
        daylight_date: SystemTime { _f: [0; 8] },
        daylight_bias: 0,
    };
    // 0 unknown, 1 standard, 2 daylight; 0xFFFFFFFF failed. UTC = local + bias.
    let which = unsafe { GetTimeZoneInformation(&mut tz) };
    let extra = match which {
        1 => tz.standard_bias,
        2 => tz.daylight_bias,
        _ => 0,
    };
    if which == u32::MAX {
        return 0;
    }
    -((tz.bias + extra) as i64) * 60
}

#[cfg(not(windows))]
fn read_offset() -> i64 {
    let out = crate::tools::command("date").arg("+%z").output().ok();
    let raw = out
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    crate::daemon::parse_offset(&raw) as i64 * 60
}

/// A UTC moment as it reads on the wall: seconds since this machine's local
/// midnight on 1 Jan 1970. Only for showing and for "which day", never for
/// storing.
fn wall(t: u64, offset: i64) -> i64 {
    t as i64 + offset
}

/// `14:02`, on the wall clock.
pub fn hhmm(t: u64, offset: i64) -> String {
    let s = wall(t, offset).rem_euclid(86_400);
    format!("{:02}:{:02}", s / 3600, (s % 3600) / 60)
}

/// The hour on the wall clock, 0–23.
pub fn hour(t: u64, offset: i64) -> u8 {
    (wall(t, offset).rem_euclid(86_400) / 3600) as u8
}

/// Which wall-clock day a moment falls on, as a day number.
pub fn day(t: u64, offset: i64) -> i64 {
    wall(t, offset).div_euclid(86_400)
}

/// The offset in force at the moment `t`, not now: a time in July read in
/// December is on summer time (28 Sep 2026 -- `offset_secs` is today's
/// offset, and every "then" was read with it).
fn offset_at(t: u64) -> i64 {
    if let Some(told) = told_offset() {
        return told;
    }
    match home_zone() {
        Some(z) => z.offset_at(t as i64),
        // This machine's zone, when it can be named, knows its rules;
        // otherwise the machine's offset now is all there is.
        None => crate::tz::suggest().map(|z| z.offset_at(t as i64)).unwrap_or_else(machine_offset_secs),
    }
}

/// The hour on this machine's clock right now-or-then, 0–23.
pub fn hour_here(t: u64) -> u8 {
    hour(t, offset_at(t))
}

/// Which day on this machine's clock a moment falls on.
pub fn day_here(t: u64) -> i64 {
    day(t, offset_at(t))
}

/// `14:02` for any moment, on the clock in force then (5 Oct 2026, audit
/// Q15: a time next week read with today's offset is an hour out once the
/// clocks change in between).
pub fn hhmm_here(t: u64) -> String {
    hhmm(t, offset_at(t))
}

/// The real moment of the local midnight that starts the day `t` falls on,
/// on your clock (`midnight_in`).
pub fn midnight_here(t: u64) -> u64 {
    midnight_in(t, &zone())
}

/// The real moment of the next local midnight after `t`'s: the end of its
/// day, which is 23 or 25 hours after the start on the days the clocks change.
pub fn next_midnight_here(t: u64) -> u64 {
    let z = zone();
    utc_of_wall_in(day(t, z.offset_at(t as i64)) + 1, 0, &z)
}

/// `secs` past local midnight on wall-clock day `day`, as a real moment, on
/// your clock (`utc_of_wall_in`).
pub fn utc_of_wall(day: i64, secs: i64) -> u64 {
    utc_of_wall_in(day, secs, &zone())
}

/// The local midnight that starts `t`'s day in `z`, with the offset in force
/// at that midnight -- which on the day the clocks change is not the one at
/// `t`.
pub fn midnight_in(t: u64, z: &crate::tz::Zone) -> u64 {
    utc_of_wall_in(day(t, z.offset_at(t as i64)), 0, z)
}

/// "5 pm on the 12th" in `z` as a moment: `secs` past midnight on wall-clock
/// day `day`, on the zone's rules for that day.
pub fn utc_of_wall_in(day: i64, secs: i64, z: &crate::tz::Zone) -> u64 {
    z.to_utc(day * 86_400 + secs).max(0) as u64
}

/// The real moment (UTC seconds) of the local midnight that starts the day
/// `t` falls on.
pub fn midnight(t: u64, offset: i64) -> u64 {
    let w = wall(t, offset);
    (w - w.rem_euclid(86_400) - offset).max(0) as u64
}

/// Day of the week on the wall clock: 0 Monday … 6 Sunday.
pub fn weekday(t: u64, offset: i64) -> u32 {
    // 1 Jan 1970 was a Thursday (3 with Monday as 0).
    ((day(t, offset) + 3).rem_euclid(7)) as u32
}

/// "It's 12:56 AM on Saturday 26 September." — the time and day on the wall
/// clock, said.
pub fn spoken_now(t: u64, offset: i64) -> String {
    let s = wall(t, offset).rem_euclid(86_400);
    let (h24, m) = (s / 3600, (s % 3600) / 60);
    let (h12, ampm) = match h24 {
        0 => (12, "AM"),
        1..=11 => (h24, "AM"),
        12 => (12, "PM"),
        _ => (h24 - 12, "PM"),
    };
    let (_, month, dom) = civil_from_days(day(t, offset));
    const DAYS: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
    const MONTHS: [&str; 12] = [
        "January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December",
    ];
    format!(
        "It's {h12}:{m:02} {ampm} on {} {dom} {}.",
        DAYS[weekday(t, offset) as usize],
        MONTHS[(month as usize).saturating_sub(1).min(11)]
    )
}

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
