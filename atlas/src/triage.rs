//! Sorting a full inbox by what it actually asks of you.
//!
//! Phase 3. Every mail client sorts by sender, date or folder, which are all
//! facts about the message rather than about you. The only question that
//! matters is **what does this need from me, and by when** — and that cuts
//! across all three.
//!
//! Nothing here sends anything. Triage reads, sorts, and drafts; sending stays
//! where it was, behind approval that's checked again at the moment of send.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub from: String,
    pub subject: String,
    pub body: String,
    pub at: u64,
    /// You're one of many.
    pub to_many: bool,
    /// It's a reply to something you sent.
    pub is_reply: bool,
    /// You've already replied.
    pub answered: bool,
}

impl From<&crate::imap::Message> for Message {
    /// A raw IMAP message, converted into the shape triage actually reads.
    /// The only judgement call here is `to_many` — "several" rather than
    /// counting exactly, since `To`/`Cc` addresses are comma-separated at
    /// the header level regardless of whether one of them is a mailing
    /// list — but two or more addressees is already the signal that
    /// matters: a question asked of eight people probably isn't yours
    /// alone to answer, and that's true whether it's eight or three.
    fn from(m: &crate::imap::Message) -> Message {
        let recipients = format!("{},{}", m.to, m.cc);
        let to_many = recipients.split(',').filter(|s| !s.trim().is_empty()).count() > 1;
        Message {
            id: m.uid.to_string(),
            from: m.from.clone(),
            subject: m.subject.clone(),
            body: m.body.clone(),
            at: parse_rfc2822(&m.date).unwrap_or(0),
            to_many,
            is_reply: !m.in_reply_to.trim().is_empty(),
            answered: m.answered,
        }
    }
}

const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

/// A minimal RFC 2822 date parser — `[Day, ]D Mon Y H:M:S [+-]ZZZZ` — just
/// enough for the header `Date:` every mail server sends, no general
/// calendar library. Days-since-epoch uses Howard Hinnant's `days_from_civil`
/// algorithm (public domain, widely used precisely because it handles leap
/// years correctly without a lookup table); the parsing around it is new.
pub(crate) fn parse_rfc2822(s: &str) -> Option<u64> {
    let s = s.trim();
    // Drop the leading "Mon, " if present; not needed once the date fields
    // are read directly.
    let s = match s.find(',') {
        Some(i) => s[i + 1..].trim(),
        None => s,
    };
    let mut parts = s.split_whitespace();
    let day: i64 = parts.next()?.parse().ok()?;
    let mon = parts.next()?.to_ascii_lowercase();
    let month_str = mon.get(..3)?;
    let month = MONTHS.iter().position(|m| *m == month_str)? as u32 + 1;
    let year_raw: i64 = parts.next()?.parse().ok()?;
    // RFC 2822 allows a two-digit year for backward compatibility with
    // RFC 822; anything below 100 is assumed to be in the current
    // century's neighbourhood the way every mail client already treats it.
    let year = if year_raw < 100 { year_raw + 2000 } else { year_raw };
    let time = parts.next()?;
    let mut t = time.split(':');
    let hour: i64 = t.next()?.parse().ok()?;
    let min: i64 = t.next()?.parse().ok()?;
    let sec: i64 = t.next().unwrap_or("0").parse().ok()?;
    // Every field within a real date and time before any arithmetic: a mail
    // or feed from anywhere can say year 2022022022022026, which overflowed
    // the sum below (found by fuzzing a feed date, tests/parsers_survive_garbage.rs).
    if !(1..=9999).contains(&year) || !(1..=31).contains(&day) || !(0..24).contains(&hour) || !(0..60).contains(&min) || !(0..=60).contains(&sec) {
        return None;
    }
    let tz = parts.next().unwrap_or("+0000");
    let tz_offset_secs = parse_tz_offset(tz).unwrap_or(0);

    let days = days_from_civil(year, month, day as u32);
    let local_secs = days * 86400 + hour * 3600 + min * 60 + sec;
    let utc_secs = local_secs - tz_offset_secs;
    if utc_secs < 0 {
        return None;
    }
    Some(utc_secs as u64)
}

fn parse_tz_offset(tz: &str) -> Option<i64> {
    if tz.len() != 5 || !tz.is_ascii() {
        return None;
    }
    let sign = match tz.as_bytes()[0] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let hh: i64 = tz[1..3].parse().ok()?;
    let mm: i64 = tz[3..5].parse().ok()?;
    Some(sign * (hh * 3600 + mm * 60))
}

/// Days since 1970-01-01, for any Gregorian calendar date. Howard
/// Hinnant's `days_from_civil` — see <http://howardhinnant.github.io/date_algorithms.html>.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// The inverse of `days_from_civil` — the calendar date for a given
/// number of days since the epoch. Same source, same reasoning: exact,
/// no lookup table, correct across the whole Gregorian calendar.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

const IMAP_MONTHS: [&str; 12] =
    ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// A date `days_ago` days before `now`, in the `DD-Mon-YYYY` format
/// IMAP's own `SEARCH SINCE` wants. `now` is a Unix timestamp — the same
/// clock everything else in this program already reads its "now" from.
pub fn imap_date(days_ago: u32, now: u64) -> String {
    let today_days = now as i64 / 86400;
    let (y, m, d) = civil_from_days(today_days - days_ago as i64);
    format!("{:02}-{}-{:04}", d, IMAP_MONTHS[(m - 1) as usize], y)
}

/// What it needs from you. Ordered by how much of your attention it deserves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Needs {
    /// Nothing. Filed.
    Nothing,
    /// Worth knowing, no action.
    Reading,
    /// You're waiting on them, not the other way round.
    TheirMove,
    /// A reply, but nothing hangs on it.
    Reply,
    /// A decision only you can make.
    Decision,
    /// Something happens or doesn't, on a date.
    Deadline,
}

impl Needs {
    pub fn what(&self) -> &'static str {
        match self {
            Needs::Nothing => "nothing",
            Needs::Reading => "worth reading",
            Needs::TheirMove => "waiting on them",
            Needs::Reply => "a reply",
            Needs::Decision => "a decision",
            Needs::Deadline => "action by a date",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Triaged {
    pub id: String,
    pub needs: Needs,
    /// One line: what it's actually about.
    pub gist: String,
    /// Why it was sorted this way, so you can disagree.
    pub because: String,
    /// A date mentioned, if there was one.
    pub by_when: Option<String>,
    /// Atlas could draft the reply.
    pub draftable: bool,
}

const ASKS_A_QUESTION: &[&str] = &[
    "could you", "can you", "would you", "let me know", "what do you think",
    "your thoughts", "please confirm", "need your", "waiting on your",
    "when you get a chance", "any update",
];

const NEEDS_DECIDING: &[&str] = &[
    "approve", "sign off", "go ahead", "which option", "decide", "your call",
    "yes or no", "do you want", "shall we", "either", "authorise", "authorize",
];

const HAS_A_DATE: &[&str] = &[
    "by friday", "by monday", "by tuesday", "by wednesday", "by thursday",
    "deadline", "due by", "before the", "expires", "end of day", "eod",
    "end of week", "eow", "closes on", "last chance", "final notice",
];

const JUST_TELLING_YOU: &[&str] = &[
    "fyi", "for your information", "no action needed", "no reply needed",
    "just so you know", "heads up", "keeping you in the loop",
];

const NOT_A_PERSON: &[&str] = &[
    "unsubscribe", "no-reply", "noreply", "newsletter", "notification@",
    "do not reply", "marketing", "promotions", "webinar", "% off",
];

/// Sort one message.
pub fn triage(m: &Message) -> Triaged {
    let t = format!("{} {}", m.subject, m.body).to_lowercase();
    let from = m.from.to_lowercase();

    // Not from a person, and nothing to do about it.
    if NOT_A_PERSON.iter().any(|p| from.contains(p) || t.contains(p)) {
        return Triaged {
            id: m.id.clone(),
            needs: Needs::Nothing,
            gist: first_line(&m.subject),
            because: "automated, and nothing in it asks anything".into(),
            by_when: None,
            draftable: false,
        };
    }

    // Already dealt with.
    if m.answered {
        return Triaged {
            id: m.id.clone(),
            needs: Needs::TheirMove,
            gist: first_line(&m.subject),
            because: "you've replied — it's with them".into(),
            by_when: None,
            draftable: false,
        };
    }

    let date = HAS_A_DATE.iter().find(|d| t.contains(**d)).map(|d| d.to_string());
    let decides = NEEDS_DECIDING.iter().any(|d| t.contains(d));
    let asks = ASKS_A_QUESTION.iter().any(|a| t.contains(a)) || m.body.contains('?');
    let telling = JUST_TELLING_YOU.iter().any(|f| t.contains(f));

    // A date attached to something that wants an answer is the only thing that
    // can go wrong by being ignored, so it sorts highest.
    let needs = if date.is_some() && (asks || decides) {
        Needs::Deadline
    } else if decides {
        Needs::Decision
    } else if telling {
        Needs::Reading
    } else if asks && !m.to_many {
        Needs::Reply
    } else if asks && m.to_many {
        // Addressed to eight people, so it probably isn't yours to answer.
        Needs::Reading
    } else if m.is_reply {
        Needs::TheirMove
    } else {
        Needs::Reading
    };

    let because = match needs {
        Needs::Deadline => format!("asks something, and mentions {}", date.clone().unwrap_or_default()),
        Needs::Decision => "asks you to decide something".into(),
        Needs::Reply => "asks you something directly".into(),
        Needs::Reading if m.to_many => "asks something, but you're one of several".into(),
        Needs::Reading if telling => "says outright it needs nothing".into(),
        Needs::TheirMove => "nothing here for you to do".into(),
        _ => "nothing in it asks anything".into(),
    };

    Triaged {
        id: m.id.clone(),
        needs,
        gist: gist_of(m),
        because,
        by_when: date,
        // Atlas will draft a reply to a plain question. It will not draft a
        // decision — that's the part only you can make.
        draftable: needs == Needs::Reply,
    }
}

/// What it's about, in a line.
///
/// From the body where possible: subjects are written to be opened, not to be
/// informative.
fn gist_of(m: &Message) -> String {
    let first = m
        .body
        .lines()
        .map(str::trim)
        .find(|l| l.len() > 20 && !l.starts_with('>') && !l.to_lowercase().starts_with("hi "))
        .unwrap_or(&m.subject);
    let cut: String = first.chars().take(90).collect();
    if first.chars().count() > 90 {
        format!("{cut}…")
    } else {
        cut
    }
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").trim().to_string()
}

/// The whole inbox, sorted by what it needs.
pub fn sort_all(messages: &[Message]) -> Vec<Triaged> {
    let mut out: Vec<Triaged> = messages.iter().map(triage).collect();
    out.sort_by_key(|b| std::cmp::Reverse(b.needs));
    out
}

/// What Atlas says about an inbox.
///
/// The count is not the point — "you have 47 emails" is what the client
/// already told you. What matters is how many actually need you.
pub fn spoken(sorted: &[Triaged]) -> String {
    let needing: Vec<&Triaged> = sorted
        .iter()
        .filter(|t| matches!(t.needs, Needs::Deadline | Needs::Decision | Needs::Reply))
        .collect();

    if needing.is_empty() {
        return format!(
            "{} in the inbox, none of it needs you.",
            sorted.len()
        );
    }
    let mut s = format!(
        "{} of {} need you.",
        needing.len(),
        sorted.len()
    );
    if let Some(first) = needing.first() {
        s.push_str(&format!(" First: {} — {}.", first.gist, first.needs.what()));
        if let Some(w) = &first.by_when {
            s.push_str(&format!(" {w}."));
        }
    }
    let draftable = needing.iter().filter(|t| t.draftable).count();
    if draftable > 0 {
        s.push_str(&format!(
            " I can draft {} of them.",
            draftable
        ));
    }
    // What can be safely ignored is as much the verdict as what can't. Naming
    // the rest as the pile you can leave unread is the whole point of the
    // sort -- otherwise the count that needs you sits inside the same inbox it
    // always did. Only reached when some of it needs you: the all-quiet case
    // returned "none of it needs you" above and has nothing to set apart.
    let waiting = can_wait(sorted);
    if !waiting.is_empty() {
        s.push_str(&format!(
            " The other {} can wait — leave {} unread.",
            waiting.len(),
            if waiting.len() == 1 { "it" } else { "them" }
        ));
    }
    s
}

/// Everything that can wait, so you can not look at it.
pub fn can_wait(sorted: &[Triaged]) -> Vec<&Triaged> {
    sorted.iter().filter(|t| t.needs <= Needs::TheirMove).collect()
}

/// You disagreed with how something was sorted. That's the correction worth
/// learning from — it's about your inbox, not inboxes in general.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Corrections {
    /// Sender to what you actually said it needs.
    pub by_sender: Vec<(String, Needs)>,
}

impl Corrections {
    pub fn note(&mut self, from: &str, actually: Needs) {
        let from = from.to_lowercase();
        match self.by_sender.iter_mut().find(|(f, _)| *f == from) {
            Some((_, n)) => *n = actually,
            None => self.by_sender.push((from, actually)),
        }
    }

    /// Apply what you've said before.
    pub fn adjust(&self, m: &Message, t: Triaged) -> Triaged {
        let from = m.from.to_lowercase();
        match self.by_sender.iter().find(|(f, _)| from.contains(f.as_str())) {
            Some((_, needs)) => Triaged {
                needs: *needs,
                because: format!("you've told me {} is {}", m.from, needs.what()),
                draftable: *needs == Needs::Reply,
                ..t
            },
            None => t,
        }
    }
}

#[cfg(test)]
mod date_and_conversion_tests {
    use super::*;

    #[test]
    fn a_normal_date_with_a_negative_offset_matches_the_real_epoch_time() {
        // date -u -d "2026-01-01 09:00:00 -0800" +%s
        assert_eq!(parse_rfc2822("Thu, 1 Jan 2026 09:00:00 -0800"), Some(1767286800));
    }

    #[test]
    fn the_unix_epoch_itself_parses_to_zero() {
        assert_eq!(parse_rfc2822("Thu, 1 Jan 1970 00:00:00 +0000"), Some(0));
    }

    #[test]
    fn a_leap_year_date_is_handled_correctly() {
        // date -u -d "2000-03-01 00:00:00 +0000" +%s -- 2000 is a leap year
        // (divisible by 400), so this exercises the era/leap-day math.
        assert_eq!(parse_rfc2822("Wed, 1 Mar 2000 00:00:00 +0000"), Some(951868800));
    }

    #[test]
    fn a_two_digit_year_is_read_as_the_2000s() {
        let a = parse_rfc2822("Thu, 1 Jan 26 09:00:00 -0800").unwrap();
        let b = parse_rfc2822("Thu, 1 Jan 2026 09:00:00 -0800").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn a_positive_offset_is_subtracted_the_right_direction() {
        // 09:00 in UTC+2 is 07:00 UTC -- an hour *earlier* than the same
        // clock time at UTC, not later. Getting the sign backwards here
        // would put every message from Europe or Asia in the wrong order
        // relative to messages from the Americas.
        let plus2 = parse_rfc2822("Thu, 1 Jan 2026 09:00:00 +0200").unwrap();
        let utc = parse_rfc2822("Thu, 1 Jan 2026 07:00:00 +0000").unwrap();
        assert_eq!(plus2, utc);
    }

    #[test]
    fn an_unparseable_date_does_not_panic_and_yields_none() {
        assert_eq!(parse_rfc2822("not a date at all"), None);
        assert_eq!(parse_rfc2822(""), None);
    }

    #[test]
    fn an_imap_message_converts_into_the_triage_shape_atlas_actually_reads() {
        let raw = crate::imap::Message {
            uid: 42,
            from: "boss@example.com".into(),
            subject: "Quick question".into(),
            date: "Thu, 1 Jan 2026 09:00:00 -0800".into(),
            to: "me@example.com".into(),
            cc: String::new(),
            in_reply_to: String::new(),
            message_id: String::new(),
            references: String::new(),
            list_unsubscribe: String::new(),
            authentication_results: String::new(),
            answered: false,
            seen: false,
            body: "Could you send the report over?".into(),
        };
        let m: Message = (&raw).into();
        assert_eq!(m.id, "42");
        assert_eq!(m.from, "boss@example.com");
        assert_eq!(m.at, 1767286800);
        assert!(!m.to_many, "one recipient should not count as several");
        assert!(!m.is_reply);
    }

    #[test]
    fn several_recipients_across_to_and_cc_together_count_as_to_many() {
        let raw = crate::imap::Message {
            to: "me@example.com".into(),
            cc: "someone-else@example.com".into(),
            ..Default::default()
        };
        let m: Message = (&raw).into();
        assert!(m.to_many, "a recipient on To plus one on Cc is still several people");
    }

    #[test]
    fn an_in_reply_to_header_marks_the_message_as_a_reply() {
        let raw = crate::imap::Message {
            in_reply_to: "<abc@example.com>".into(),
            ..Default::default()
        };
        let m: Message = (&raw).into();
        assert!(m.is_reply);
    }

    #[test]
    fn the_answered_flag_carries_straight_through() {
        let raw = crate::imap::Message { answered: true, ..Default::default() };
        let m: Message = (&raw).into();
        assert!(m.answered);
    }

    #[test]
    fn days_from_civil_matches_known_epoch_boundaries() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(1970, 1, 2), 1);
        assert_eq!(days_from_civil(1969, 12, 31), -1);
        // 2000 is a leap year (divisible by 400) -- day 60 exists.
        assert_eq!(days_from_civil(2000, 2, 29), days_from_civil(2000, 3, 1) - 1);
    }

    #[test]
    fn parse_tz_offset_reads_sign_hours_and_minutes() {
        assert_eq!(parse_tz_offset("+0000"), Some(0));
        assert_eq!(parse_tz_offset("-0800"), Some(-8 * 3600));
        assert_eq!(parse_tz_offset("+0530"), Some(5 * 3600 + 30 * 60));
        assert_eq!(parse_tz_offset("garbage"), None);
    }

    #[test]
    fn imap_date_sixty_days_before_a_known_date_matches_the_real_calendar() {
        // date -u -d "2026-01-01 -60 days" +%d-%b-%Y
        let jan_1_2026 = 1767225600; // date -u -d "2026-01-01" +%s
        assert_eq!(imap_date(60, jan_1_2026), "02-Nov-2025");
    }

    #[test]
    fn imap_date_crosses_a_leap_year_boundary_correctly() {
        // date -u -d "2026-03-01 -1 days" +%d-%b-%Y -- 2026 is not a leap
        // year, so the day before March 1st is the 28th, not the 29th.
        let mar_1_2026 = 1767225600 + 59 * 86400;
        assert_eq!(imap_date(1, mar_1_2026), "28-Feb-2026");
    }

    #[test]
    fn imap_date_at_zero_days_is_today() {
        let jan_1_2026 = 1767225600;
        assert_eq!(imap_date(0, jan_1_2026), "01-Jan-2026");
    }
}
