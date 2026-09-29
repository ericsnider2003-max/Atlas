//! Atlas keeps your time, not UTC's (24 Sep 2026).
//!
//! Every time-of-day decision in Atlas — "tomorrow at 3", reminders, the
//! greeting, quiet hours, when your day turns over — was taken on UTC's clock.
//! There was no way to read the machine's zone, and a comment in the daemon
//! said so. For Eric in Pacific time that is seven hours out: "remind me at 3"
//! went off at eight in the morning.
//!
//! These pin the fix on a Pacific clock (UTC−7), whatever zone the tests run in.

use atlas::calendar::{resolve_when_in, When};
use atlas::localclock::{day, hhmm, hour, midnight, weekday};

const PDT: i64 = -7 * 3600;
/// Wednesday 23 Sep 2026, 16:00 UTC — 09:00 in the morning in Pacific time.
const WED_9AM_PACIFIC: u64 = 1_790_179_200;

#[test]
fn the_moment_used_here_is_what_it_says() {
    assert_eq!(hhmm(WED_9AM_PACIFIC, 0), "16:00");
    assert_eq!(hhmm(WED_9AM_PACIFIC, PDT), "09:00");
    assert_eq!(weekday(WED_9AM_PACIFIC, PDT), 2, "Wednesday");
}

#[test]
fn tomorrow_at_3_is_3pm_tomorrow_where_you_are() {
    let w: When = resolve_when_in("remind me tomorrow at 3pm", WED_9AM_PACIFIC, &atlas::tz::Zone::fixed(PDT)).unwrap();
    assert_eq!(hhmm(w.start, PDT), "15:00", "3pm on your clock");
    assert_eq!(weekday(w.start, PDT), 3, "Thursday, where you are");
    // The real moment is 22:00 UTC. On UTC's clock this used to be 15:00
    // UTC — eight in the morning for you.
    assert_eq!(hhmm(w.start, 0), "22:00");
    assert_eq!(w.start - WED_9AM_PACIFIC, 30 * 3600);
}

#[test]
fn today_means_your_today_even_late_in_the_evening() {
    // 22:00 Wednesday in Pacific time is already Thursday 05:00 UTC. "Today
    // at 11pm" must still mean Wednesday.
    let late = WED_9AM_PACIFIC + 13 * 3600;
    assert_eq!(hhmm(late, PDT), "22:00");
    let w = resolve_when_in("today at 11pm", late, &atlas::tz::Zone::fixed(PDT)).unwrap();
    assert_eq!(day(w.start, PDT), day(late, PDT), "still your Wednesday");
    assert_eq!(hhmm(w.start, PDT), "23:00");
    assert_eq!(w.start - late, 3600, "an hour from now, not a day and an hour");
}

#[test]
fn a_named_weekday_counts_from_your_day() {
    // At 22:00 Wednesday Pacific (Thursday in UTC), "on Thursday" is tomorrow.
    let late = WED_9AM_PACIFIC + 13 * 3600;
    let w = resolve_when_in("dentist on thursday at 9am", late, &atlas::tz::Zone::fixed(PDT)).unwrap();
    assert_eq!(weekday(w.start, PDT), 3);
    assert_eq!(hhmm(w.start, PDT), "09:00");
    assert!(w.start - late < 86_400, "tomorrow, not next week");
}

#[test]
fn an_event_says_its_time_on_your_clock() {
    let mut c = atlas::calendar::Calendar::default();
    let w = resolve_when_in("call tomorrow at 3pm", WED_9AM_PACIFIC, &atlas::tz::Zone::fixed(PDT)).unwrap();
    c.add("call", w, None, WED_9AM_PACIFIC);
    let e = &c.occurrences_between(w.start, w.end)[0];
    assert_eq!(hhmm(e.start, PDT), "15:00", "the event is at 3 pm on your clock");
    let said = e.say_when_in(&atlas::tz::Zone::fixed(PDT));
    // Since the merge (26 Sep) the time is followed by the zone's name, so
    // it says which clock it's on.
    assert!(said.contains(" 15:00"), "{said}");
    assert!(!said.contains('Z'), "no UTC marker on a time meant for you: {said}");
}

#[test]
fn the_hour_and_the_day_are_yours() {
    assert_eq!(hour(WED_9AM_PACIFIC, PDT), 9);
    // Midnight is your midnight: 07:00 UTC.
    let m = midnight(WED_9AM_PACIFIC, PDT);
    assert_eq!(hhmm(m, PDT), "00:00");
    assert_eq!(hhmm(m, 0), "07:00");
    assert_eq!(WED_9AM_PACIFIC - m, 9 * 3600);
    // East of UTC works the same way.
    assert_eq!(hhmm(midnight(WED_9AM_PACIFIC, 5 * 3600 + 1800), 0), "18:30");
}

#[test]
fn nothing_left_reads_the_hour_straight_off_utc() {
    // The pattern the old code used everywhere: seconds mod a day, over an
    // hour. Market sessions and fx rollovers are defined in UTC and are
    // allowed; everything that decides something about *your* day is not.
    let allowed = ["src/market/", "src/fxday.rs", "src/rollover.rs", "src/localclock.rs", "src/digest.rs"];
    let mut offenders = Vec::new();
    for entry in walk("src") {
        let path = entry.to_string_lossy().replace('\\', "/");
        if allowed.iter().any(|a| path.contains(a)) {
            continue;
        }
        let text = std::fs::read_to_string(&entry).unwrap_or_default();
        for (n, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            if code.contains("% 86_400) / 3600") || code.contains("% 86400) / 3600") || code.contains("/ 3600) % 24") {
                offenders.push(format!("{path}:{}: {}", n + 1, line.trim()));
            }
        }
    }
    assert!(offenders.is_empty(), "the hour read on UTC's clock:\n  {}", offenders.join("\n  "));
}

fn walk(dir: &str) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(p.to_str().unwrap()));
        } else if p.extension().map(|x| x == "rs").unwrap_or(false) {
            out.push(p);
        }
    }
    out
}

#[test]
fn the_suite_runs_on_utc_and_the_program_reads_the_machine() {
    // `.cargo/config.toml` pins the clock for tests; the built program never
    // sees that file.
    let cfg = std::fs::read_to_string(".cargo/config.toml").unwrap();
    assert!(cfg.contains("ATLAS_CLOCK_OFFSET = \"0\""));
    let src = std::fs::read_to_string("src/localclock.rs").unwrap();
    assert!(src.contains("GetTimeZoneInformation"), "Windows is asked directly");
}
