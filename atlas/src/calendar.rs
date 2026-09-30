//! Your own calendar, kept here, and the bridge to the one on your phone.
//!
//! The point of building this in rather than leaning on someone's app is that
//! it works with nothing connected: events live in Atlas's own store, offline,
//! and are yours whether or not a phone is paired. The native phone calendar
//! — the one already on your iPhone or Android, whatever app you use — is a
//! *sync target*, not a dependency. Nobody has to install a particular app to
//! use this.
//!
//! Two halves, kept apart on purpose:
//!
//! - **The store** (`Calendar`): add, query, remove. Pure, offline, tested.
//! - **The bridge** (`merge_from_phone` / `for_phone`): reconcile a batch the
//!   phone read from its native calendar, and hand back the Atlas-made events
//!   the phone should add to it. The actual EventKit (iOS) / CalendarProvider
//!   (Android) calls live in the phone app — the same boundary the Android
//!   client sits behind — so this tree holds the seam, not the platform code.
//!
//! Times are Unix seconds, like the rest of the daemon. Civil-date maths goes
//! through `market::time`, the one date library already in the tree.

use serde::{Deserialize, Serialize};

use crate::market::time::{Utc, MS_PER_DAY};

/// Where an event came from. Kept so a sync can tell its own events from the
/// phone's and never clobber one with the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Made here, in Atlas.
    Atlas,
    /// Read from the native calendar on your phone.
    Phone,
    /// Read from an `.ics` file — an invite from Outlook, Google or Apple, or
    /// a business partner's calendar export (`import_ics`).
    File,
}

/// What kind of thing a calendar entry is.
///
/// A meeting is time with other people; a time block is time you've reserved
/// for yourself — focus, deep work, heads-down. Both are booked time (a
/// meeting scheduled over a block still clashes), but they are shown apart so
/// a day of reserved focus doesn't read as a day of meetings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum EventKind {
    #[default]
    Meeting,
    TimeBlock,
}

/// How an event repeats. A personal calendar without repeating events isn't
/// one — a standup, the gym, a weekly one-to-one. The four common patterns
/// keep their own names; anything else ("the last Friday of every month",
/// "every other Tuesday", "the 15th of every month") is an RFC 5545 rule
/// carried as its text, read by `recur` — the same rule every `.ics` file
/// carries, so an invite from anyone else's calendar means the same here.
///
/// A rule is only ever read from words `recur::from_plain` is sure of, and
/// only when the request says "every"/"each"/"monthly"/"yearly": a one-off
/// wrongly made to recur is a calendar full of things you never scheduled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Repeat {
    /// A one-off. The default, and what every event before this field was.
    #[default]
    Once,
    /// Every day.
    Daily,
    /// Monday to Friday.
    Weekdays,
    /// The same weekday every week — the weekday the first one falls on.
    Weekly,
    /// Anything else, as an RRULE (`FREQ=MONTHLY;BYDAY=-1FR`).
    Rule(String),
}

impl Repeat {
    /// How it reads on the calendar, in words. Empty for a one-off.
    pub fn label(&self) -> String {
        match self {
            Repeat::Once => String::new(),
            Repeat::Daily => "every day".into(),
            Repeat::Weekdays => "every weekday".into(),
            Repeat::Weekly => "every week".into(),
            Repeat::Rule(r) => match crate::recur::Rule::parse(r) {
                Ok(rule) => rule.describe(),
                // Stored text that no longer parses is said as what it is
                // rather than hidden: the event still exists.
                Err(_) => format!("repeats ({r})"),
            },
        }
    }

    /// The general rule, when it is one.
    pub fn rule(&self) -> Option<crate::recur::Rule> {
        match self {
            Repeat::Rule(r) => crate::recur::Rule::parse(r).ok(),
            _ => None,
        }
    }
}

/// Read a repeat pattern from a scheduling request, or `Once` if none is named.
///
/// Narrow and literal, like `resolve_when`: it reads the phrases people use —
/// "every day", "every weekday", "every week", "every Monday" — and nothing it
/// isn't sure about becomes a repeat, because a one-off wrongly made to recur
/// is a calendar full of things you never scheduled. Past the four common
/// ones, "every other Tuesday", "the last Friday of every month" and "the
/// 15th of each month" become a `Rule` — but only when the request says
/// every/each/monthly/yearly out loud.
pub fn repeat_from(text: &str) -> Repeat {
    let t = text.to_lowercase();
    if t.contains("every weekday") || t.contains("each weekday") || t.contains("on weekdays") {
        return Repeat::Weekdays;
    }
    // Checked before the plain "every day"/"every week" readers, which would
    // otherwise take "every 2 weeks" or "every other week" as every week.
    let says_repeat = ["every ", "each ", "monthly", "yearly", "annually"].iter().any(|c| t.contains(c));
    // Only the words that make a repeat something other than daily/weekly
    // hand it to the general rule: a count right after "every" ("every 2
    // weeks"), "other", an ordinal, or a month/year cadence. A clock time
    // ("at 6") is not a count.
    let ws: Vec<&str> = t.split_whitespace().collect();
    let counted = ws.windows(2).any(|p| {
        matches!(p[0], "every" | "each")
            && (p[1].parse::<u32>().is_ok_and(|n| n > 1) || ["two", "three", "four", "other"].contains(&p[1]))
    });
    let ordinal = ws.iter().any(|w| ["first", "second", "third", "fourth", "last"].contains(w));
    let simple_every = !counted && !ordinal && !t.contains("month") && !t.contains("year");
    if says_repeat && !simple_every {
        if let Some(rule) = crate::recur::from_plain(&t) {
            return Repeat::Rule(rule.to_rrule());
        }
    }
    if t.contains("every day") || t.contains("everyday") || t.contains("each day") || t.contains("daily") {
        return Repeat::Daily;
    }
    if t.contains("every week") || t.contains("weekly") {
        return Repeat::Weekly;
    }
    // "every Monday", "each Friday" — a named weekday means weekly on that day,
    // and `resolve_when` already lands the first one on it.
    for d in ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"] {
        if t.contains(&format!("every {d}")) || t.contains(&format!("each {d}")) {
            return Repeat::Weekly;
        }
    }
    Repeat::Once
}

/// One thing on your calendar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: u64,
    pub title: String,
    /// Unix seconds.
    pub start: u64,
    /// Unix seconds. For an all-day event this is the end of the day.
    pub end: u64,
    pub all_day: bool,
    /// Where it is, if you said.
    #[serde(default)]
    pub place: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    /// Which side of the personal/business firewall this belongs to. Defaults
    /// to personal — every event that predates this field, and every plain
    /// "schedule lunch tomorrow", is yours. An event that names a business you
    /// have is filed on that business's side (see `space_for_request`), so the
    /// calendar can show one combined view or one business at a time without a
    /// second calendar to keep in step.
    #[serde(default)]
    pub space: crate::earned::Space,
    /// Meeting or a time block you reserved for yourself. Defaults to a
    /// meeting — what every event before this field was, and what a plain
    /// "schedule …" makes; a "block off …" makes a time block.
    #[serde(default)]
    pub kind: EventKind,
    /// How it repeats. Defaults to `Once` — every event before this field, and
    /// every plain "schedule lunch tomorrow", is a one-off.
    #[serde(default)]
    pub repeat: Repeat,
    /// Minutes before it starts to remind you, if you asked for a reminder.
    /// `None` means no reminder — the default, since a calendar that pings you
    /// about everything is one you mute. For a repeat, the lead applies to each
    /// occurrence.
    #[serde(default)]
    pub remind_before_mins: Option<u32>,
    pub source: Source,
    /// The native calendar's own id for this event, when it came from the
    /// phone. This is how a re-sync updates the same event instead of adding a
    /// second copy.
    #[serde(default)]
    pub phone_key: Option<String>,
    pub created: u64,
    /// Occurrences of a repeating event that were cancelled one at a time
    /// (an `.ics` EXDATE). Unix seconds of the occurrence start.
    #[serde(default)]
    pub except: Vec<u64>,
    /// The wall clock a repeating event keeps: "standup every weekday at 9"
    /// said in Los Angeles is 9:00 there in July and in December, which is two
    /// different UTC hours. `None` (every event before this field, and every
    /// one set with no time zone chosen) repeats in UTC, as before.
    #[serde(default)]
    pub zone: Option<String>,
}

impl Event {
    fn overlaps(&self, start: u64, end: u64) -> bool {
        self.start < end && start < self.end
    }

    /// A line naming when it is, in plain words — and how it repeats, if it
    /// does, so a recurring event never reads as a single date.
    ///
    /// On your clock, not UTC's: an event you made for "3" reads as 15:00
    /// where you are (`localclock::zone`: the zone you chose, or this
    /// machine's). It used to read "15:00Z" — honest about being UTC, and
    /// seven hours out for Eric in Pacific time (fixed on the third chat's
    /// line, 24 Sep; merged 26 Sep into `say_when_in`, which also names the
    /// zone). On a UTC clock it reads as it always did.
    pub fn say_when(&self) -> String {
        self.say_when_in(&crate::localclock::zone())
    }

    /// The UTC form: "2026-10-01 17:00Z".
    fn say_when_utc(&self) -> String {
        let rep = match &self.repeat {
            Repeat::Once => String::new(),
            r => format!(" ({})", r.label()),
        };
        let s = Utc::from_ms(self.start as i64 * 1000);
        if self.all_day {
            return format!("{} (all day){rep}", s.say_date());
        }
        format!("{} {:02}:{:02}Z{rep}", s.say_date(), s.hour, s.minute)
    }

    /// `say_when` on your clock: "2026-10-01 17:00 PDT". An event that keeps
    /// another zone's wall clock (an invite from Osaka) also says its own
    /// time, "(09:00 JST there)", since that is the one the other person
    /// will quote. UTC reads exactly as `say_when` always has.
    pub fn say_when_in(&self, home: &crate::tz::Zone) -> String {
        let at = self.start as i64;
        if home.is_utc() && self.zone.is_none() {
            return self.say_when_utc();
        }
        let rep = match &self.repeat {
            Repeat::Once => String::new(),
            r => format!(" ({})", r.label()),
        };
        let c = crate::civil::Civil::from_local(home.to_local(at));
        let date = format!("{:04}-{:02}-{:02}", c.year, c.month, c.day);
        if self.all_day {
            return format!("{date} (all day){rep}");
        }
        let theirs = match self.zone.as_deref().and_then(crate::tz::Zone::named) {
            Some(z) if z.offset_at(at) != home.offset_at(at) => {
                let t = crate::civil::Civil::from_local(z.to_local(at));
                format!(" ({:02}:{:02} {} there)", t.hour, t.minute, z.abbreviation_at(at))
            }
            _ => String::new(),
        };
        format!("{date} {:02}:{:02} {}{theirs}{rep}", c.hour, c.minute, home.abbreviation_at(at))
    }
}

/// A resolved time: the answer to "when?", so a caller can add an event or say
/// it didn't understand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct When {
    pub start: u64,
    pub end: u64,
    pub all_day: bool,
}

/// How long a plain event runs when you didn't say, in seconds.
const DEFAULT_MINS: u64 = 60;
const DAY_SECS: u64 = 86_400;

/// Turn words into a time, or admit it couldn't.
///
/// Read by `when` — days, dates, clock times, offsets, ranges and lengths the
/// way people say them — and taken only when `when` is sure. Anything it
/// can't read, or reads two ways ("next Monday" said on a Sunday, 3/4), is
/// `None`, so the caller asks rather than inventing a time. Guessing at a
/// time you'll be somewhere is the one thing a calendar must not do.
///
/// Before round 9 this read only "today"/"tomorrow"/a weekday and a clock
/// time, and took a bare "at 3" as three in the morning. Measured on the
/// same 106 phrases (`tests/when_corpus.rs`): 28 right, 22 wrong and 56 not
/// read, against `when`'s 106, 0 and 0.
pub fn resolve_when(text: &str, now: u64) -> Option<When> {
    let p = crate::when::parse(text, now)?;
    // A clock time with no day ("at 5pm") could be today or tomorrow; a
    // calendar asks rather than picks.
    if !p.sure || !p.day_said {
        return None;
    }
    if p.all_day {
        return Some(When { start: p.start, end: p.start + DAY_SECS, all_day: true });
    }
    let mins = p.mins.or_else(|| duration_mins(&text.to_lowercase())).unwrap_or(DEFAULT_MINS);
    Some(When { start: p.start, end: p.start + mins * 60, all_day: false })
}

/// `resolve_when` for a real moment `now` on a clock `offset` seconds east
/// of UTC, returning real moments: "tomorrow at 3" is 3 pm tomorrow *there*.
///
/// The third chat's form (24 Sep 2026), kept for its callers and its tests
/// (`tests/your_clock.rs`): `resolve_when` itself works in local seconds and
/// its callers convert, which is how round 9's `when` parser reads text.
pub fn resolve_when_in(text: &str, now: u64, zone: &crate::tz::Zone) -> Option<When> {
    // Each moment is converted back with the offset in force *then* (28 Sep
    // 2026): this took a fixed offset, so "the Monday after the clocks
    // change at 9" came out an hour out. A fixed offset is `Zone::fixed`.
    let local_now = zone.to_local(now as i64).max(0) as u64;
    let w = resolve_when(text, local_now)?;
    let back = |t: u64| zone.to_utc(t as i64).max(0) as u64;
    Some(When { start: back(w.start), end: back(w.end), all_day: w.all_day })
}

/// Read a reminder lead-time from a scheduling request, in minutes, or `None`.
///
/// "remind me 10 minutes before", "with a 30-minute reminder", "an hour
/// before", "remind me a day before". Narrow and literal like the rest: if it
/// can't read a clear lead, there's no reminder rather than a guessed one.
pub fn reminder_from(text: &str) -> Option<u32> {
    // The event's own length ("for 2 hours") is not the reminder's lead:
    // "meeting at 3 for 2 hours, remind me an hour before" is an hour.
    let t = without_length(&text.to_lowercase());
    // Must actually mention reminding / "before", so "for 30 minutes" (a
    // duration) is never misread as a reminder.
    if !t.contains("remind") && !t.contains("before") {
        return None;
    }
    // "a day before" / "the day before".
    if t.contains("day before") {
        return Some(24 * 60);
    }
    // "an hour before" / "1 hour before" / "2 hours before".
    if let Some(mins) = number_before(&t, "hour").map(|n| n * 60).or_else(|| {
        if t.contains("hour before") || t.contains("hour reminder") {
            Some(60)
        } else {
            None
        }
    }) {
        return Some(mins);
    }
    // "N minutes before" / "N-minute reminder" / "N min before".
    if let Some(mins) = number_before(&t, "min") {
        return Some(mins);
    }
    // A bare "remind me" with no interval: a sensible default of ten minutes.
    if t.contains("remind me") || t.contains("with a reminder") || t.contains("set a reminder") {
        return Some(10);
    }
    None
}

/// The integer immediately before a unit word ("30" in "30 minutes",
/// "30-minute", "30 min"). `None` if there isn't one.
///
/// Tries every occurrence of the unit, because the substring can hide inside
/// another word — "min" lives inside "re**min**d" and "re**min**der" — and only
/// an occurrence with a digit in front of it is the one that means a number.
/// `t` with every "for N minutes/hours" (and "for an hour", "for half an
/// hour") taken out: what's left can be read for a reminder's lead.
fn without_length(t: &str) -> String {
    let w: Vec<&str> = t.split_whitespace().collect();
    let unit = |x: &str| {
        let x = x.trim_matches(|c: char| !c.is_alphanumeric());
        x.starts_with("hour") || x.starts_with("hr") || x.starts_with("min")
    };
    let mut out: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < w.len() {
        if w[i] == "for" {
            // for N unit | for an/a unit | for half an unit | for N-unit
            let n = w.get(i + 1).copied().unwrap_or("");
            let skip = if n == "half" && w.get(i + 3).map(|u| unit(u)).unwrap_or(false) {
                4
            } else if (n.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) || n == "an" || n == "a")
                && w.get(i + 2).map(|u| unit(u)).unwrap_or(false)
            {
                3
            } else if n.contains('-') && n.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) && unit(n.split('-').last().unwrap_or("")) {
                2
            } else {
                0
            };
            if skip > 0 {
                i += skip;
                continue;
            }
        }
        out.push(w[i]);
        i += 1;
    }
    out.join(" ")
}

fn number_before(text: &str, unit: &str) -> Option<u32> {
    let mut from = 0;
    while let Some(rel) = text[from..].find(unit) {
        let idx = from + rel;
        let digits: String = text[..idx]
            .trim_end_matches(|c: char| c == ' ' || c == '-')
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        if let Ok(n) = digits.parse::<u32>() {
            return Some(n);
        }
        from = idx + unit.len();
    }
    None
}

/// The first occurrence of a possibly-repeating request.
///
/// For a one-off this is exactly `resolve_when`. For a repeat named without an
/// explicit day — "standup every weekday at 9", "gym every day at 7am" — the
/// series starts today at that clock time (and a weekday repeat that would
/// start on a weekend begins on the next Monday instead). A request with no
/// clock time at all still returns `None`, so the caller asks rather than
/// inventing one.
pub fn resolve_recurring_when(text: &str, now: u64, repeat: &Repeat) -> Option<When> {
    // A general rule decides its own first day: "the last Friday of every
    // month at 4", said on a Wednesday, starts on Friday, not today — and
    // `resolve_when` would read "friday" as this Friday, which is only
    // right by accident.
    if let Some(rule) = repeat.rule() {
        let t = text.to_lowercase();
        let minute_of_day = clock_minute(&t)?;
        let anchor = start_of_day(now) + minute_of_day as u64 * 60;
        let anchor = if anchor < now { anchor + DAY_SECS } else { anchor };
        let first = rule.first_at_or_after(anchor as i64)? as u64;
        let mins = duration_mins(&t).unwrap_or(DEFAULT_MINS);
        return Some(When { start: first, end: first + mins * 60, all_day: false });
    }
    if let Some(w) = resolve_when(text, now) {
        return Some(w);
    }
    if *repeat == Repeat::Once {
        return None;
    }
    let t = text.to_lowercase();
    let minute_of_day = clock_minute(&t)?;
    let mut day_start = start_of_day(now);
    if *repeat == Repeat::Weekdays {
        // Don't begin a weekday series on a Saturday or Sunday.
        while Utc::from_ms(day_start as i64 * 1000).weekday() >= 5 {
            day_start += DAY_SECS;
        }
    }
    let start = day_start + minute_of_day as u64 * 60;
    let mins = duration_mins(&t).unwrap_or(DEFAULT_MINS);
    Some(When { start, end: start + mins * 60, all_day: false })
}

/// A clean title from a scheduling request: the leading verb and the trailing
/// time words trimmed off, so "schedule lunch tomorrow at 12" becomes "lunch".
/// Falls back to the whole phrase if trimming would leave nothing.
pub fn event_title(request: &str) -> String {
    let mut words: Vec<&str> = request.split_whitespace().collect();
    // Leading verb.
    const VERBS: &[&str] = &[
        "schedule", "add", "put", "book", "set", "create", "new", "make", "remind", "block",
        "reserve", "hold",
    ];
    while let Some(first) = words.first() {
        if VERBS.contains(&first.to_lowercase().trim_matches(|c: char| !c.is_alphanumeric())) {
            words.remove(0);
        } else {
            break;
        }
    }
    // Trailing time phrase: drop from the first time-word to the end.
    const TIME_WORDS: &[&str] = &[
        "today", "tonight", "tomorrow", "monday", "tuesday", "wednesday", "thursday", "friday",
        "saturday", "sunday", "at", "on", "for", "this", "next",
    ];
    // Also where the time is said some other way: "in 3 days", "from 2 to
    // 4", "oct 3", "3pm", "10/15", "the 14th" -- found by asking `when`
    // whether the words from there on name a time on their own.
    let starts_time = |i: usize| {
        let w = words[i].to_lowercase();
        let w = w.trim_matches(|c: char| !c.is_alphanumeric() && c != '/' && c != ':');
        if TIME_WORDS.contains(&w) {
            return true;
        }
        let digit_led = w.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false);
        let lead = matches!(w, "in" | "from" | "between" | "tmrw" | "tmr" | "noon" | "midnight" | "eod" | "end")
            || digit_led
            // A day or a month name: "friday", "oct" (read as "oct 1").
            // ("the 1" is the 1st, so "the" is left to the check above.)
            || (w != "the" && crate::when::parse(&format!("{w} 1"), 20_000 * DAY_SECS).map(|p| p.all_day && p.day_said).unwrap_or(false));
        lead && crate::when::parse(&words[i..words.len().min(i + 4)].join(" "), 20_000 * DAY_SECS).is_some()
    };
    // The first place a time starts -- but never the very first word, which
    // would leave no title ("3d print session tomorrow" is not about "3d").
    if let Some(cut) = (1..words.len()).find(|&i| starts_time(i)) {
        words.truncate(cut);
    } else if let Some(cut) = words.iter().position(|w| {
        TIME_WORDS.contains(&w.to_lowercase().trim_matches(|c: char| !c.is_alphanumeric()))
    }) {
        words.truncate(cut);
    }
    // "team sync the last Friday of every month" cuts at "Friday" and would
    // leave "team sync the last"; the words that only introduce a repeat go too.
    const REPEAT_LEAD: &[&str] =
        &["the", "every", "each", "other", "last", "first", "second", "third", "fourth", "fifth"];
    while words.len() > 1
        && words.last().is_some_and(|w| REPEAT_LEAD.contains(&w.to_lowercase().as_str()))
    {
        words.pop();
    }
    let title = words.join(" ").trim().to_string();
    if title.is_empty() {
        request.trim().to_string()
    } else {
        title
    }
}

/// Which side of the firewall a scheduling request belongs on.
///
/// An event that names a business you actually have is filed on that
/// business's side; everything else is yours. Bounded on purpose — it only
/// recognises a business you have already set up — so "schedule lunch with
/// Sam" is never mistaken for business, and a stray word never invents one.
/// The business's own spelling wins, not the caller's, so "northwind" in a
/// request files under "Northwind". When more than one matches, the longest
/// name wins, so "Northwind West" beats a bare "Northwind".
pub fn space_for_request(request: &str, businesses: &[String]) -> crate::earned::Space {
    let low = request.to_lowercase();
    let mut best: Option<&String> = None;
    for b in businesses {
        if names_it(&low, &b.to_lowercase()) && best.map(|cur| b.len() > cur.len()).unwrap_or(true) {
            best = Some(b);
        }
    }
    match best {
        Some(b) => crate::earned::Space::Business(b.clone()),
        None => crate::earned::Space::Personal,
    }
}

/// Whether a scheduling request is reserving focus time rather than booking a
/// meeting.
///
/// Recognised by the words people actually use to wall time off — "block off",
/// "time block", "deep work", "heads down", "focus time", "no meetings" — and
/// by a bare leading "block" ("block two hours tomorrow"). Deliberately not
/// triggered by "focus" alone, so "schedule a focus group" stays a meeting.
/// Everything it doesn't recognise is a meeting, which is the safe default.
pub fn kind_for_request(request: &str) -> EventKind {
    let low = request.to_lowercase();
    const CUES: &[&str] = &[
        "block off",
        "block out",
        "time block",
        "time-block",
        "timeblock",
        "focus time",
        "focus block",
        "deep work",
        "heads down",
        "heads-down",
        "no meetings",
        "do not disturb",
    ];
    if CUES.iter().any(|c| low.contains(c)) {
        return EventKind::TimeBlock;
    }
    if low.split_whitespace().next() == Some("block") {
        return EventKind::TimeBlock;
    }
    EventKind::Meeting
}

/// Whether `needle` appears in `haystack` as a whole word rather than glued
/// inside another — so a business called "Ace" is not found inside "space".
/// Both are expected already-lowercased.
fn names_it(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    // By characters, not bytes: stepping one byte on from a match began a
    // slice inside "é" or "李" and panicked (27 Sep 2026), and a byte test
    // for "letter" read every accented letter as a gap.
    let mut from = 0;
    while let Some(pos) = haystack[from..].find(needle) {
        let start = from + pos;
        let end = start + needle.len();
        let before_ok = haystack[..start].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
        let after_ok = haystack[end..].chars().next().is_none_or(|c| !c.is_alphanumeric());
        if before_ok && after_ok {
            return true;
        }
        from = start + haystack[start..].chars().next().map_or(1, char::len_utf8);
    }
    false
}

/// Midnight (UTC) of the day a timestamp falls in, in Unix seconds.
pub fn start_of_day(secs: u64) -> u64 {
    let days = (secs as i64 * 1000).div_euclid(MS_PER_DAY);
    (days * MS_PER_DAY / 1000) as u64
}




/// Read a clock time out of the words: "at 3", "3pm", "15:00", "9:30am".
/// Returns minutes past midnight.
///
/// A bare number is only a time if it carries am/pm, a `:MM`, or sits right
/// after "at" — so "for 30 minutes" is never mistaken for half past midnight.
fn clock_minute(t: &str) -> Option<u32> {
    // The clock time `when` finds, whatever day it's on.
    // Any day far from the epoch will do: only the time of day is read.
    let p = crate::when::parse(t, 20_000 * DAY_SECS)?;
    (!p.all_day).then(|| ((p.start % DAY_SECS) / 60) as u32)
}

/// Read a run of ASCII digits as a number, returning it and the index after.
fn take_number(bytes: &[u8], start: usize) -> (u32, usize) {
    let mut n = 0u32;
    let mut i = start;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        n = n.saturating_mul(10).saturating_add((bytes[i] - b'0') as u32);
        i += 1;
    }
    (n, i)
}

/// Read "for 30 minutes" / "for 2 hours" / "90 min".
fn duration_mins(t: &str) -> Option<u64> {
    let idx = t.find("for ")?;
    let rest = &t[idx + 4..];
    let (n, j) = take_number(rest.as_bytes(), 0);
    if n == 0 {
        return None;
    }
    let unit = rest[j..].trim_start();
    if unit.starts_with("hour") || unit.starts_with("hr") {
        Some(n as u64 * 60)
    } else if unit.starts_with("min") {
        Some(n as u64)
    } else {
        None
    }
}

/// Settings for the calendar and its bridge to the phone.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct CalendarConfig {
    pub enabled: bool,
    /// Keep the native phone calendar in step: read its events in, and offer
    /// Atlas's events out for it to add. The platform calls (EventKit /
    /// CalendarProvider) live in the phone app; this flag is what that app
    /// checks before syncing.
    pub sync_native: bool,
    /// How far ahead "what's coming up" looks, in days.
    pub horizon_days: u32,
}

impl Default for CalendarConfig {
    fn default() -> Self {
        CalendarConfig { enabled: true, sync_native: true, horizon_days: 7 }
    }
}

/// Your calendar.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Calendar {
    events: Vec<Event>,
    next_id: u64,
}

const FILE: &str = "calendar";

impl Calendar {
    pub fn load(store: &crate::store::Store) -> Calendar {
        store.load::<Calendar>(FILE)
    }

    pub fn save(&self, store: &crate::store::Store) -> crate::error::Result<()> {
        store.save(FILE, self)
    }

    /// Put something on the calendar. Returns its id.
    pub fn add(&mut self, title: &str, when: When, place: Option<String>, now: u64) -> u64 {
        self.add_in(title, when, place, crate::earned::Space::Personal, now)
    }

    /// The same, filing the event on a chosen side of the firewall. `add` is
    /// this with `Space::Personal`, which is the right default for a plain
    /// "schedule lunch tomorrow"; a business event comes through here.
    pub fn add_in(
        &mut self,
        title: &str,
        when: When,
        place: Option<String>,
        space: crate::earned::Space,
        now: u64,
    ) -> u64 {
        self.add_full(title, when, place, space, EventKind::Meeting, Repeat::Once, now)
    }

    /// The full form: firewall side, whether it's a meeting or a time block you
    /// reserved for yourself, and how it repeats. `add` and `add_in` are this
    /// with the common defaults (personal, meeting, one-off); a "block off …"
    /// comes through here as a `TimeBlock`, and "every weekday" as a `Repeat`.
    pub fn add_full(
        &mut self,
        title: &str,
        when: When,
        place: Option<String>,
        space: crate::earned::Space,
        kind: EventKind,
        repeat: Repeat,
        now: u64,
    ) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.events.push(Event {
            id,
            title: title.trim().to_string(),
            start: when.start,
            end: when.end,
            all_day: when.all_day,
            place,
            note: None,
            space,
            kind,
            repeat,
            remind_before_mins: None,
            source: Source::Atlas,
            phone_key: None,
            created: now,
            except: Vec::new(),
            zone: None,
        });
        id
    }

    /// Pin a repeating event to a zone's wall clock (see `Event::zone`). A
    /// one-off, or UTC, needs nothing: its single instant is already exact.
    pub fn keep_wall_clock(&mut self, id: u64, zone: &crate::tz::Zone) {
        if let Some(e) = self.events.iter_mut().find(|e| e.id == id) {
            if e.repeat != Repeat::Once && !zone.is_utc() {
                e.zone = Some(zone.id());
            }
        }
    }

    /// Set (or clear) the reminder lead-time on an event. True if it was there.
    pub fn set_reminder(&mut self, id: u64, mins: Option<u32>) -> bool {
        if let Some(e) = self.events.iter_mut().find(|e| e.id == id) {
            e.remind_before_mins = mins;
            true
        } else {
            false
        }
    }

    /// How long after an event started its reminder is still said.
    pub const REMIND_LATE_SECS: u64 = 15 * 60;

    /// The occurrences whose reminder is due right now — start is still ahead
    /// but within its lead time. Repeating events are expanded, so a daily
    /// standup reminds each day. The caller tracks which it has already spoken
    /// (by id and occurrence start) so none fires twice.
    pub fn due_reminders(&self, now: u64) -> Vec<Event> {
        // A reminder fires while now is in [start - lead, start). The largest
        // lead we support is a day, so a day-and-change window covers every one
        // without scanning further than it must.
        //
        // And for `REMIND_LATE_SECS` after it started (29 Sep 2026): a laptop
        // asleep through the whole lead time, or Atlas paused, used to drop
        // the reminder altogether; now it is said late ("started 4 minutes
        // ago") rather than not at all.
        let window_end = now + 26 * 3600;
        self.occurrences_between(now.saturating_sub(Self::REMIND_LATE_SECS), window_end)
            .into_iter()
            .filter(|e| {
                let Some(mins) = e.remind_before_mins else { return false };
                let lead = mins as u64 * 60;
                e.start + Self::REMIND_LATE_SECS > now && e.start.saturating_sub(lead) <= now
            })
            .collect()
    }

    /// Take one off. True if it was there.
    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.events.len();
        self.events.retain(|e| e.id != id);
        self.events.len() != before
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// The next thing, if any. Series-level — for the moment right after
    /// scheduling, where the caller knows the id it just made.
    /// The event with this id, wherever it falls. Used to say back what was
    /// just booked: `next()` is the soonest event overall, which is a
    /// different one whenever something else is already booked earlier.
    pub fn event(&self, id: u64) -> Option<&Event> {
        self.events.iter().find(|e| e.id == id)
    }

    pub fn next(&self, now: u64) -> Option<&Event> {
        self.events.iter().filter(|e| e.end > now).min_by_key(|e| e.start)
    }

    /// Every actual occurrence in a window, repeating events expanded. A
    /// one-off yields itself if it overlaps; a repeating one yields a dated
    /// copy for each time it happens in `[from, to)`, with the same duration and
    /// title. This is what a person means by "what's on" — the standup on each
    /// of the days it's on, not one row that says "every weekday".
    pub fn occurrences_between(&self, from: u64, to: u64) -> Vec<Event> {
        self.occurrences_on(from, to, crate::localclock::offset_secs())
    }

    /// `occurrences_between`, with the weekday of a series that keeps no zone
    /// of its own read on a clock `off` seconds east of UTC.
    ///
    /// Merged 26 Sep 2026 from two fixes for the same bug. The third chat
    /// read a weekly event's day on this machine's clock (a Monday-evening
    /// event in Pacific time is a Tuesday in UTC, and used to repeat on
    /// Tuesdays); round 3 gave an event its own zone to keep. An event with
    /// a zone is expanded on that zone's wall clock below, which already is
    /// local, so it passes an offset of 0 to avoid shifting twice.
    fn occurrences_on(&self, from: u64, to: u64, off: i64) -> Vec<Event> {
        let mut out = Vec::new();
        for e in &self.events {
            // A series that keeps a wall clock: expand it on that clock, then
            // bring each occurrence back to UTC. The window is widened by a
            // day each side for the shift and trimmed again afterwards.
            if let (false, Some(z)) = (e.repeat == Repeat::Once, e.zone.as_deref().and_then(crate::tz::Zone::named)) {
                let dur = e.end.saturating_sub(e.start);
                let mut local = e.clone();
                local.zone = None;
                local.start = z.to_local(e.start as i64).max(0) as u64;
                local.end = local.start + dur;
                local.except = e.except.iter().map(|x| z.to_local(*x as i64).max(0) as u64).collect();
                let one = Calendar { events: vec![local], ..Default::default() };
                for mut occ in one.occurrences_on(from.saturating_sub(DAY_SECS), to + DAY_SECS, 0) {
                    occ.start = z.to_utc(occ.start as i64).max(0) as u64;
                    occ.end = occ.start + dur;
                    occ.zone = e.zone.clone();
                    if occ.start < to && occ.end > from {
                        out.push(occ);
                    }
                }
                continue;
            }
            if e.repeat == Repeat::Once {
                if e.start < to && e.end > from {
                    out.push(e.clone());
                }
                continue;
            }
            let dur = e.end.saturating_sub(e.start);
            if let Some(rule) = e.repeat.rule() {
                // The general rule expands itself; the day-walk below only
                // knows the four fixed patterns.
                let mut series = crate::recur::Series::new(e.start as i64, rule);
                series.exdates = e.except.iter().map(|x| *x as i64).collect();
                let lo = from.saturating_sub(dur) as i64;
                for s in series.between(lo, to as i64, 4000) {
                    let s = s as u64;
                    if s + dur > from && s < to {
                        let mut occ = e.clone();
                        occ.start = s;
                        occ.end = s + dur;
                        out.push(occ);
                    }
                }
                continue;
            }
            let series_wd = crate::localclock::weekday(e.start, off);
            // Start walking near the window rather than from the series origin,
            // so a daily event set up a year ago doesn't cost a year of steps.
            let mut s = e.start;
            if s + dur <= from {
                let whole_days = (from - (s + dur)) / DAY_SECS;
                s += whole_days * DAY_SECS;
            }
            // Bounded: the window is bounded and the step is a day, so this can
            // only run for as many days as the window is wide.
            let mut guard = 0u32;
            while s < to && guard < 4000 {
                guard += 1;
                let occ_end = s + dur;
                let wd = crate::localclock::weekday(s, off);
                let counts = match e.repeat {
                    Repeat::Daily => true,
                    Repeat::Weekdays => wd < 5,
                    Repeat::Weekly => wd == series_wd,
                    Repeat::Once | Repeat::Rule(_) => false,
                };
                if counts && occ_end > from && s < to && !e.except.contains(&s) {
                    let mut occ = e.clone();
                    occ.start = s;
                    occ.end = occ_end;
                    out.push(occ);
                }
                s += DAY_SECS;
            }
        }
        out.sort_by_key(|e| e.start);
        out
    }

    /// The same as `clashes`, but with repeating events expanded — so a new
    /// booking is checked against every occurrence in its window, not just
    /// events whose single stored slot happens to overlap.
    pub fn clashes_expanded(&self, start: u64, end: u64) -> Vec<Event> {
        let mut v: Vec<Event> = self
            .occurrences_between(start, end)
            .into_iter()
            .filter(|e| !e.all_day && e.overlaps(start, end))
            .collect();
        v.sort_by_key(|e| e.start);
        v
    }

    // ----- the bridge to the phone -----

    /// Fold in a batch the phone read from its native calendar. An event whose
    /// `phone_key` we've seen updates the one we have; a new key adds one; our
    /// own (`Atlas`) events are never touched by this. Returns how many were
    /// added or changed.
    pub fn merge_from_phone(&mut self, incoming: Vec<Event>, now: u64) -> usize {
        let mut changed = 0;
        for mut ev in incoming {
            ev.source = Source::Phone;
            let Some(key) = ev.phone_key.clone() else { continue };
            match self.events.iter_mut().find(|e| e.phone_key.as_deref() == Some(&key)) {
                Some(existing) => {
                    if existing.start != ev.start
                        || existing.end != ev.end
                        || existing.title != ev.title
                    {
                        existing.title = ev.title;
                        existing.start = ev.start;
                        existing.end = ev.end;
                        existing.all_day = ev.all_day;
                        existing.place = ev.place;
                        changed += 1;
                    }
                }
                None => {
                    self.next_id += 1;
                    ev.id = self.next_id;
                    ev.created = now;
                    self.events.push(ev);
                    changed += 1;
                }
            }
        }
        changed
    }

    /// A whole window the phone read (H7): merge what's there, and take out
    /// the phone's events in that window that the phone no longer has (you
    /// deleted it there). Only `Phone` events inside the window are ever
    /// removed, so a phone reading a narrower window, or an Atlas-made or
    /// `.ics` event, is never lost to it. Returns (added or changed, removed).
    pub fn sync_from_phone(&mut self, incoming: Vec<Event>, from: u64, to: u64, now: u64) -> (usize, usize) {
        let keys: std::collections::BTreeSet<String> = incoming.iter().filter_map(|e| e.phone_key.clone()).collect();
        let before = self.events.len();
        self.events.retain(|e| {
            !(e.source == Source::Phone
                && e.start >= from
                && e.start < to
                && e.phone_key.as_ref().is_some_and(|k| !keys.contains(k)))
        });
        let removed = before - self.events.len();
        (self.merge_from_phone(incoming, now), removed)
    }

    /// The Atlas-made events the phone should add to its native calendar —
    /// everything born here that hasn't come from the phone. The phone app
    /// writes these through EventKit / CalendarProvider.
    pub fn for_phone(&self) -> Vec<&Event> {
        self.events.iter().filter(|e| e.source == Source::Atlas).collect()
    }

    // ----- files other calendars read -----

    /// Read the events of an `.ics` file in — an Outlook invite, a Google or
    /// Apple export, a business partner's calendar. Keyed by each event's own
    /// UID (kept as `ics:<uid>` in the outside-key field, which a phone key
    /// can never collide with), so importing the same file again updates
    /// rather than duplicates. A repeat the file states as an RRULE is kept as
    /// that rule, with its cancelled occurrences.
    ///
    /// Times are converted to UTC, the calendar's own clock: `Z` as is, a
    /// `TZID=` through the file's own VTIMEZONE or the zone tables in `tz`,
    /// and a floating time (or a TZID nothing knows) as `home`, your zone.
    /// The TZIDs that had to fall back to `home` come back so they can be
    /// said, not silently assumed.
    pub fn import_ics(&mut self, text: &str, now: u64, home: &crate::tz::Zone) -> std::result::Result<(usize, Vec<String>), String> {
        let mut changed = 0;
        let (events, zones) = crate::vformat::events_in(text, home)?;
        for ev in events {
            let key = format!("ics:{}", if ev.uid.is_empty() { &ev.summary } else { &ev.uid });
            let start = ev.start.max(0) as u64;
            let end = ev.end.map(|e| e.max(0) as u64).unwrap_or(if ev.all_day { start + DAY_SECS } else { start + DEFAULT_MINS * 60 });
            let repeat = match &ev.rule {
                Some(r) => Repeat::Rule(r.to_rrule()),
                None => Repeat::Once,
            };
            let place = (!ev.location.is_empty()).then(|| ev.location.clone());
            let note = (!ev.description.is_empty()).then(|| ev.description.clone());
            let except: Vec<u64> = ev.exdates.iter().map(|x| (*x).max(0) as u64).collect();
            match self.events.iter_mut().find(|e| e.phone_key.as_deref() == Some(key.as_str())) {
                Some(existing) => {
                    let before = existing.clone();
                    existing.title = ev.summary.clone();
                    existing.start = start;
                    existing.end = end;
                    existing.all_day = ev.all_day;
                    existing.place = place;
                    existing.note = note;
                    existing.repeat = repeat;
                    existing.except = except;
                    existing.zone = ev.zone.clone();
                    if *existing != before {
                        changed += 1;
                    }
                }
                None => {
                    self.next_id += 1;
                    self.events.push(Event {
                        id: self.next_id,
                        title: ev.summary.clone(),
                        start,
                        end,
                        all_day: ev.all_day,
                        place,
                        note,
                        space: crate::earned::Space::Personal,
                        kind: EventKind::Meeting,
                        repeat,
                        remind_before_mins: None,
                        source: Source::File,
                        phone_key: Some(key),
                        created: now,
                        except,
                        zone: ev.zone.clone(),
                    });
                    changed += 1;
                }
            }
        }
        Ok((changed, zones.unknown.into_inner()))
    }

    /// The calendar as an `.ics` file any other calendar can open. Repeats go
    /// out as RRULEs (the four fixed patterns included), so a series stays a
    /// series on the other side.
    pub fn to_ics(&self, now: u64) -> String {
        let events: Vec<crate::vformat::Event> = self
            .events
            .iter()
            .map(|e| {
                let rule = match &e.repeat {
                    Repeat::Once => None,
                    Repeat::Daily => crate::recur::Rule::parse("FREQ=DAILY").ok(),
                    Repeat::Weekdays => crate::recur::Rule::parse("FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR").ok(),
                    Repeat::Weekly => crate::recur::Rule::parse("FREQ=WEEKLY").ok(),
                    Repeat::Rule(_) => e.repeat.rule(),
                };
                crate::vformat::Event {
                    uid: match e.phone_key.as_deref().and_then(|k| k.strip_prefix("ics:")) {
                        Some(u) => u.to_string(),
                        None => format!("atlas-{}-{}", e.id, e.created),
                    },
                    summary: e.title.clone(),
                    start: e.start as i64,
                    end: Some(e.end as i64),
                    all_day: e.all_day,
                    location: e.place.clone().unwrap_or_default(),
                    description: e.note.clone().unwrap_or_default(),
                    rule,
                    exdates: e.except.iter().map(|x| *x as i64).collect(),
                    zone: e.zone.clone(),
                }
            })
            .collect();
        crate::vformat::calendar(&events, now as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A fixed Thursday for stable maths: 2021-01-07 00:00:00Z is a Thursday.
    const THU: u64 = 1_609_977_600;

    #[test]
    fn resolves_a_day_and_a_clock_time() {
        // Said at 9 in the morning: at midnight itself "tomorrow" could mean
        // later today, and is asked about (round 10).
        let w = resolve_when("lunch tomorrow at 12", THU + 9 * 3600).unwrap();
        assert!(!w.all_day);
        // Tomorrow (Friday) at 12:00.
        let s = Utc::from_ms(w.start as i64 * 1000);
        assert_eq!((s.hour, s.minute), (12, 0));
        assert_eq!(w.end - w.start, DEFAULT_MINS * 60);
    }

    #[test]
    fn reads_pm_and_a_duration() {
        let w = resolve_when("call today at 3pm for 30 minutes", THU).unwrap();
        let s = Utc::from_ms(w.start as i64 * 1000);
        assert_eq!(s.hour, 15);
        assert_eq!(w.end - w.start, 30 * 60);
    }

    #[test]
    fn a_bare_day_is_all_day() {
        let w = resolve_when("dentist tomorrow", THU + 9 * 3600).unwrap();
        assert!(w.all_day);
        assert_eq!(w.end - w.start, DAY_SECS);
    }

    #[test]
    fn a_weekday_name_is_the_next_one() {
        // From Thursday, "monday" is the coming Monday (4 days on).
        let w = resolve_when("standup monday at 9am", THU).unwrap();
        assert_eq!(start_of_day(w.start), THU + 4 * DAY_SECS);
    }

    #[test]
    fn the_same_weekday_means_next_week_not_today() {
        // From Thursday, "thursday" is 7 days on, never today.
        let w = resolve_when("review thursday at 10am", THU).unwrap();
        assert_eq!(start_of_day(w.start), THU + 7 * DAY_SECS);
    }

    #[test]
    fn no_day_means_it_could_not_read_it() {
        assert!(resolve_when("do the thing at 5pm", THU).is_none());
        assert!(resolve_when("something vague", THU).is_none());
    }

    #[test]
    fn add_query_and_remove() {
        let mut c = Calendar::default();
        let w = resolve_when("meeting today at 2pm", THU).unwrap();
        let id = c.add("Meeting", w, Some("office".into()), THU);
        assert_eq!(c.occurrences_between(start_of_day(THU), start_of_day(THU) + DAY_SECS).len(), 1);
        assert_eq!(c.next(THU).unwrap().title, "Meeting");
        assert!(c.remove(id));
        assert!(c.is_empty());
    }

    #[test]
    fn a_clash_is_seen() {
        let mut c = Calendar::default();
        let w = resolve_when("a today at 2pm for 60 minutes", THU).unwrap();
        c.add("A", w, None, THU);
        // 2:30pm overlaps the 2–3 event.
        let w2 = resolve_when("b today at 2:30pm for 60 minutes", THU).unwrap();
        assert_eq!(c.clashes_expanded(w2.start, w2.end).len(), 1);
        // 4pm does not.
        let w3 = resolve_when("c today at 4pm for 30 minutes", THU).unwrap();
        assert_eq!(c.clashes_expanded(w3.start, w3.end).len(), 0);
    }

    #[test]
    fn upcoming_respects_the_horizon() {
        let mut c = Calendar::default();
        c.add("soon", resolve_when("today at 5pm", THU).unwrap(), None, THU);
        c.add("far", resolve_when("today at 5pm", THU + 30 * DAY_SECS).unwrap(), None, THU);
        assert_eq!(c.occurrences_between(THU, THU + 7 * DAY_SECS).len(), 1);
    }

    #[test]
    fn a_phone_event_merges_and_updates_not_duplicates() {
        let mut c = Calendar::default();
        let ev = Event {
            id: 0,
            title: "Sync me".into(),
            start: THU + 3600,
            end: THU + 7200,
            all_day: false,
            place: None,
            note: None,
            space: crate::earned::Space::Personal,
            kind: EventKind::Meeting,
            repeat: Repeat::Once,
            remind_before_mins: None,
            source: Source::Phone,
            phone_key: Some("ABC-123".into()),
            except: Vec::new(),
            zone: None,
            created: 0,
        };
        assert_eq!(c.merge_from_phone(vec![ev.clone()], THU), 1);
        assert_eq!(c.len(), 1);
        // Same key, new time: updates in place, no second copy.
        let mut moved = ev;
        moved.start = THU + 5400;
        moved.end = THU + 9000;
        assert_eq!(c.merge_from_phone(vec![moved], THU), 1);
        assert_eq!(c.len(), 1);
        assert_eq!(c.next(THU).unwrap().start, THU + 5400);
    }

    #[test]
    fn only_atlas_events_go_out_to_the_phone() {
        let mut c = Calendar::default();
        c.add("mine", resolve_when("today at 1pm", THU).unwrap(), None, THU);
        c.merge_from_phone(
            vec![Event {
                id: 0,
                title: "theirs".into(),
                start: THU + 3600,
                end: THU + 7200,
                all_day: false,
                place: None,
                note: None,
                space: crate::earned::Space::Personal,
                kind: EventKind::Meeting,
                repeat: Repeat::Once,
                remind_before_mins: None,
                source: Source::Phone,
                phone_key: Some("K".into()),
                except: Vec::new(),
                zone: None,
                created: 0,
            }],
            THU,
        );
        let out = c.for_phone();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, "mine");
    }

    // ----- the firewall on the calendar -----

    use crate::earned::Space;

    #[test]
    fn a_plain_event_is_personal() {
        let mut c = Calendar::default();
        let id = c.add("lunch", resolve_when("today at 1pm", THU).unwrap(), None, THU);
        let e = c.events.iter().find(|e| e.id == id).unwrap();
        assert_eq!(e.space, Space::Personal, "an ordinary event is yours by default");
    }

    #[test]
    fn a_business_event_is_filed_on_that_side() {
        let mut c = Calendar::default();
        let id = c.add_in(
            "review",
            resolve_when("today at 2pm", THU).unwrap(),
            None,
            Space::Business("Northwind".into()),
            THU,
        );
        let e = c.events.iter().find(|e| e.id == id).unwrap();
        assert_eq!(e.space, Space::Business("Northwind".into()));
    }

    #[test]
    fn one_view_is_everything_and_a_filter_is_one_side() {
        let mut c = Calendar::default();
        c.add("gym", resolve_when("today at 7am", THU).unwrap(), None, THU);
        c.add_in(
            "client call",
            resolve_when("today at 3pm", THU).unwrap(),
            None,
            Space::Business("Northwind".into()),
            THU,
        );
        // The combined view has both; nothing is hidden by default.
        let day = c.occurrences_between(THU, THU + DAY_SECS);
        assert_eq!(day.len(), 2);
        // Filtering is the deliberate act.
        assert_eq!(day.iter().filter(|e| e.space == Space::Personal).count(), 1);
        let biz: Vec<_> =
            day.iter().filter(|e| e.space == Space::Business("Northwind".into())).collect();
        assert_eq!(biz.len(), 1);
        assert_eq!(biz[0].title, "client call");
    }

    #[test]
    fn a_request_naming_a_business_you_have_is_filed_there() {
        let businesses = vec!["Northwind".to_string()];
        assert_eq!(
            space_for_request("schedule the Northwind review tomorrow at 2", &businesses),
            Space::Business("Northwind".into()),
        );
        // The business's own spelling wins, not the caller's.
        assert_eq!(
            space_for_request("northwind sync monday at 9", &businesses),
            Space::Business("Northwind".into()),
        );
    }

    #[test]
    fn a_request_that_names_no_business_stays_personal() {
        let businesses = vec!["Northwind".to_string()];
        assert_eq!(
            space_for_request("lunch with Sam tomorrow at noon", &businesses),
            Space::Personal,
        );
        // No businesses set up at all: everything is personal.
        assert_eq!(space_for_request("Northwind review", &[]), Space::Personal);
    }

    #[test]
    fn a_business_name_glued_inside_a_word_is_not_a_match() {
        // "Ace" must not be found inside "space".
        let businesses = vec!["Ace".to_string()];
        assert_eq!(
            space_for_request("clear some space tomorrow at 4", &businesses),
            Space::Personal,
        );
    }

    #[test]
    fn the_longest_matching_business_wins() {
        let businesses = vec!["Northwind".to_string(), "Northwind West".to_string()];
        assert_eq!(
            space_for_request("Northwind West standup friday at 9", &businesses),
            Space::Business("Northwind West".into()),
        );
    }

    // ----- meetings vs time blocks -----

    #[test]
    fn a_plain_event_is_a_meeting() {
        let mut c = Calendar::default();
        let id = c.add("lunch", resolve_when("today at 1pm", THU).unwrap(), None, THU);
        let e = c.events.iter().find(|e| e.id == id).unwrap();
        assert_eq!(e.kind, EventKind::Meeting, "an ordinary event is a meeting");
    }

    #[test]
    fn a_reserved_block_is_a_time_block() {
        let mut c = Calendar::default();
        let id = c.add_full(
            "deep work",
            resolve_when("today at 9am for 2 hours", THU).unwrap(),
            None,
            Space::Personal,
            EventKind::TimeBlock,
            Repeat::Once,
            THU,
        );
        let e = c.events.iter().find(|e| e.id == id).unwrap();
        assert_eq!(e.kind, EventKind::TimeBlock);
        // Still booked time: a meeting over it clashes, the same as any event.
        assert_eq!(c.clashes_expanded(e.start, e.end).len(), 1);
    }

    #[test]
    fn the_words_that_reserve_focus_time_are_read_as_a_block() {
        for r in [
            "block off two hours tomorrow at 9",
            "block out the morning tomorrow at 8",
            "deep work tomorrow at 10 for 2 hours",
            "focus time tomorrow at 3",
            "no meetings friday at 1",
        ] {
            assert_eq!(kind_for_request(r), EventKind::TimeBlock, "{r} should be a time block");
        }
    }

    #[test]
    fn an_ordinary_request_stays_a_meeting() {
        for r in [
            "schedule the review tomorrow at 2",
            "lunch with Sam tomorrow at noon",
            // "focus" alone must not trigger it.
            "schedule a focus group monday at 10",
        ] {
            assert_eq!(kind_for_request(r), EventKind::Meeting, "{r} should stay a meeting");
        }
    }

    // ----- reminders -----

    #[test]
    fn a_reminder_lead_is_read_from_the_words() {
        assert_eq!(reminder_from("dentist tomorrow at 9, remind me 15 minutes before"), Some(15));
        assert_eq!(reminder_from("call at 3pm with a 30-minute reminder"), Some(30));
        assert_eq!(reminder_from("meeting at 2, remind me an hour before"), Some(60));
        assert_eq!(reminder_from("flight friday, remind me a day before"), Some(24 * 60));
        assert_eq!(reminder_from("standup at 9, remind me"), Some(10));
        // A plain duration is not a reminder.
        assert_eq!(reminder_from("lunch at noon for 30 minutes"), None);
        assert_eq!(reminder_from("gym at 7am"), None);
    }

    #[test]
    fn a_reminder_is_due_only_inside_its_lead_window() {
        let mut c = Calendar::default();
        // Meeting at THU 2pm with a 15-minute reminder.
        let w = resolve_when("today at 2pm", THU).unwrap();
        let id = c.add("meeting", w, None, THU);
        assert!(c.set_reminder(id, Some(15)));
        let start = w.start;
        // An hour before: not yet due.
        assert!(c.due_reminders(start - 3600).is_empty());
        // Ten minutes before (inside the 15-minute lead): due.
        assert_eq!(c.due_reminders(start - 600).len(), 1);
        // After it's started: not due.
        assert!(c.due_reminders(start + 60).is_empty());
        // An event with no reminder set never comes due.
        let mut c2 = Calendar::default();
        c2.add("no reminder", w, None, THU);
        assert!(c2.due_reminders(start - 600).is_empty());
    }

    #[test]
    fn a_repeat_reminds_on_each_occurrence() {
        let mut c = Calendar::default();
        let w = resolve_when("today at 9am", THU).unwrap();
        let id = c.add_full("standup", w, None, Space::Personal, EventKind::Meeting, Repeat::Daily, THU);
        assert!(c.set_reminder(id, Some(10)));
        // Five minutes before today's 9am: due.
        assert_eq!(c.due_reminders(w.start - 300).len(), 1);
        // Five minutes before tomorrow's 9am: also due (a different occurrence).
        assert_eq!(c.due_reminders(w.start + DAY_SECS - 300).len(), 1);
    }

    // ----- recurrence -----

    #[test]
    fn a_repeat_is_read_from_the_words_or_left_a_one_off() {
        assert_eq!(repeat_from("standup every weekday at 9"), Repeat::Weekdays);
        assert_eq!(repeat_from("gym every day at 7am"), Repeat::Daily);
        assert_eq!(repeat_from("daily review at 5"), Repeat::Daily);
        assert_eq!(repeat_from("one to one every week on monday"), Repeat::Weekly);
        assert_eq!(repeat_from("book club every thursday at 6"), Repeat::Weekly);
        // Nothing repeating named — a one-off, never guessed into a series.
        assert_eq!(repeat_from("lunch with Sam tomorrow at noon"), Repeat::Once);
        // Past the four: a general rule, only when "every"/"each" is said.
        assert_eq!(repeat_from("team sync the last friday of every month at 4"), Repeat::Rule("FREQ=MONTHLY;BYDAY=-1FR".into()));
        assert_eq!(repeat_from("gym every other tuesday at 7"), Repeat::Rule("FREQ=WEEKLY;INTERVAL=2;BYDAY=TU".into()));
        assert_eq!(repeat_from("rent on the 1st of each month at 9"), Repeat::Rule("FREQ=MONTHLY;BYMONTHDAY=1".into()));
        assert_eq!(repeat_from("dinner the last friday at 7"), Repeat::Once);
    }

    #[test]
    fn a_general_rule_starts_on_its_own_first_day_and_expands() {
        // THU 2021-01-07. "the last friday of every month at 4" -> Fri 2021-01-29 16:00.
        let r = repeat_from("team sync the last friday of every month at 4pm");
        let w = resolve_recurring_when("team sync the last friday of every month at 4pm", THU, &r).unwrap();
        assert_eq!(Utc::from_ms(w.start as i64 * 1000).say_date(), Utc::from_ms((THU + 22 * DAY_SECS) as i64 * 1000).say_date());
        let mut c = Calendar::default();
        c.add_full(&event_title("team sync the last friday of every month at 4pm"), w, None, crate::earned::Space::Personal, EventKind::Meeting, r, THU);
        assert_eq!(c.events[0].title, "team sync");
        let occ = c.occurrences_between(THU, THU + 100 * DAY_SECS);
        assert_eq!(occ.len(), 3); // Jan 29, Feb 26, Mar 26 (Apr 30 is past the window)
        assert!(occ[0].say_when().contains("every month on last Friday"));
    }

    #[test]
    fn a_daily_event_shows_on_every_day_of_the_window() {
        let mut c = Calendar::default();
        // THU at 9am, every day. Over a 3-day window from THU, three occurrences.
        let w = resolve_when("today at 9am", THU).unwrap();
        c.add_full("standup", w, None, Space::Personal, EventKind::Meeting, Repeat::Daily, THU);
        let occ = c.occurrences_between(start_of_day(THU), start_of_day(THU) + 3 * DAY_SECS);
        assert_eq!(occ.len(), 3, "a daily event should appear on each of the three days");
        // Each occurrence is a day apart, same title.
        assert!(occ.iter().all(|e| e.title == "standup"));
        assert_eq!(occ[1].start - occ[0].start, DAY_SECS);
    }

    #[test]
    fn a_weekday_event_skips_the_weekend() {
        let mut c = Calendar::default();
        // THU is a weekday. Over a 7-day window from THU: Thu, Fri, (skip Sat,
        // Sun), Mon, Tue, Wed = 5 occurrences.
        let w = resolve_when("today at 9am", THU).unwrap();
        c.add_full("standup", w, None, Space::Personal, EventKind::Meeting, Repeat::Weekdays, THU);
        let occ = c.occurrences_between(start_of_day(THU), start_of_day(THU) + 7 * DAY_SECS);
        assert_eq!(occ.len(), 5, "five weekdays in the seven-day window from Thursday");
        // None of them falls on a Saturday or Sunday.
        assert!(occ.iter().all(|e| Utc::from_ms(e.start as i64 * 1000).weekday() < 5));
    }

    #[test]
    fn a_weekly_event_lands_on_the_same_weekday_only() {
        let mut c = Calendar::default();
        let w = resolve_when("today at 9am", THU).unwrap();
        c.add_full("review", w, None, Space::Personal, EventKind::Meeting, Repeat::Weekly, THU);
        // Two weeks: exactly two occurrences, seven days apart, both on THU's
        // weekday.
        let occ = c.occurrences_between(start_of_day(THU), start_of_day(THU) + 14 * DAY_SECS);
        assert_eq!(occ.len(), 2);
        assert_eq!(occ[1].start - occ[0].start, 7 * DAY_SECS);
    }

    #[test]
    fn a_booking_onto_a_future_occurrence_of_a_repeat_clashes() {
        let mut c = Calendar::default();
        // A daily 9–10 standup.
        let w = resolve_when("today at 9am for 60 minutes", THU).unwrap();
        c.add_full("standup", w, None, Space::Personal, EventKind::Meeting, Repeat::Daily, THU);
        // Book something at 9:30 THREE days later — the single stored slot
        // wouldn't overlap, but that day's occurrence does.
        let later = resolve_when("today at 9:30am for 30 minutes", THU + 3 * DAY_SECS).unwrap();
        assert_eq!(
            c.clashes_expanded(later.start, later.end).len(),
            1,
            "the repeat's occurrence three days out should be seen as a clash"
        );
    }

    #[test]
    fn a_repeat_says_how_it_repeats_when_read_back() {
        let mut c = Calendar::default();
        let w = resolve_when("today at 9am", THU).unwrap();
        c.add_full("standup", w, None, Space::Personal, EventKind::Meeting, Repeat::Weekdays, THU);
        let e = &c.events[0];
        assert!(e.say_when().contains("every weekday"), "the recurrence must show: {}", e.say_when());
    }

    #[test]
    fn an_event_saved_before_the_firewall_existed_loads_as_personal() {
        // A stored event with no `space` field must read back as yours, not
        // fail to load and not land on some business by accident.
        let json = r#"{
            "id": 3, "title": "old one", "start": 100, "end": 200,
            "all_day": false, "source": "atlas", "created": 0
        }"#;
        let e: Event = serde_json::from_str(json).unwrap();
        assert_eq!(e.space, Space::Personal);
        assert_eq!(e.kind, EventKind::Meeting, "an event with no kind loads as a meeting");
        assert_eq!(e.repeat, Repeat::Once, "an event with no repeat loads as a one-off");
    }
}

/// The phone's side of H7, as it travels: what the phone read from its own
/// calendars (never the "Atlas" one it writes to), and the window it read.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct PhoneBatch {
    pub from: u64,
    pub to: u64,
    #[serde(default)]
    pub events: Vec<PhoneEvent>,
}

/// One event from the phone's calendar.
#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize, PartialEq)]
pub struct PhoneEvent {
    /// The phone's own id for it (EventKit's calendarItemIdentifier plus the
    /// start, for a repeat; Android's instance id).
    pub key: String,
    pub title: String,
    pub start: u64,
    pub end: u64,
    #[serde(default)]
    pub all_day: bool,
    #[serde(default)]
    pub place: Option<String>,
}

/// The largest batch taken: a phone's month, with room to spare.
pub const PHONE_BATCH_MAX: usize = 2_000;

impl PhoneBatch {
    /// The events as Atlas keeps them. Nonsense (no key, ends before it
    /// starts, a window wider than a year) is dropped rather than kept.
    pub fn events(&self, now: u64) -> Vec<Event> {
        if self.to <= self.from || self.to - self.from > 400 * 86_400 {
            return Vec::new();
        }
        self.events
            .iter()
            .take(PHONE_BATCH_MAX)
            .filter(|e| !e.key.is_empty() && e.key.len() <= 256 && e.end >= e.start)
            .map(|e| Event {
                id: 0,
                title: e.title.chars().take(300).collect(),
                start: e.start,
                end: e.end,
                all_day: e.all_day,
                place: e.place.clone().filter(|p| !p.trim().is_empty()),
                note: None,
                space: crate::earned::Space::Personal,
                kind: EventKind::Meeting,
                repeat: Repeat::Once,
                remind_before_mins: None,
                source: Source::Phone,
                phone_key: Some(format!("phone:{}", e.key)),
                created: now,
                except: Vec::new(),
                zone: None,
            })
            .collect()
    }
}

#[cfg(test)]
mod phone_sync_tests {
    use super::*;

    fn ev(key: &str, title: &str, start: u64) -> PhoneEvent {
        PhoneEvent { key: key.into(), title: title.into(), start, end: start + 3600, ..PhoneEvent::default() }
    }

    #[test]
    fn a_phone_window_adds_changes_and_removes_only_its_own() {
        let mut c = Calendar::default();
        let mine = c.add("Atlas-made", When { start: 5_000, end: 6_000, all_day: false }, None, 1);
        let b = PhoneBatch { from: 0, to: 100_000, events: vec![ev("a", "Dentist", 10_000), ev("b", "Gym", 20_000)] };
        assert_eq!(c.sync_from_phone(b.events(1), b.from, b.to, 1), (2, 0));
        // Again, with Gym moved and Dentist deleted on the phone.
        let b = PhoneBatch { from: 0, to: 100_000, events: vec![ev("b", "Gym", 30_000)] };
        assert_eq!(c.sync_from_phone(b.events(2), b.from, b.to, 2), (1, 1));
        let titles: Vec<&str> = c.events.iter().map(|e| e.title.as_str()).collect();
        assert!(titles.contains(&"Gym") && !titles.contains(&"Dentist"), "{titles:?}");
        assert!(c.events.iter().any(|e| e.id == mine), "an Atlas-made event was taken out by a phone sync");
        // A narrower window leaves a phone event outside it alone.
        let b = PhoneBatch { from: 0, to: 10, events: vec![] };
        assert_eq!(c.sync_from_phone(b.events(3), b.from, b.to, 3), (0, 0));
        assert!(c.events.iter().any(|e| e.title == "Gym"));
        assert_eq!(c.for_phone().len(), 1, "only Atlas-made events go back to the phone");
    }

    #[test]
    fn nonsense_from_a_phone_is_dropped() {
        let b = PhoneBatch { from: 100, to: 50, events: vec![ev("a", "x", 1)] };
        assert!(b.events(1).is_empty(), "a window that ends before it starts");
        let mut bad = ev("", "no key", 1);
        let b = PhoneBatch { from: 0, to: 100, events: vec![bad.clone()] };
        assert!(b.events(1).is_empty());
        bad.key = "k".into();
        bad.end = 0;
        bad.start = 10;
        assert!(PhoneBatch { from: 0, to: 100, events: vec![bad] }.events(1).is_empty());
    }
}
