//! Read and write `.vcf` contacts and `.ics` calendars — the two file formats
//! every phone, mail client and calendar already speaks.
//!
//! **Sources:** RFC 6350 (vCard 4.0) and RFC 5545 (iCalendar): content lines
//! `NAME;PARAM=VALUE:VALUE`, folded at 75 octets with CRLF + one space,
//! text escaping of `\\ \; \, \n`. `Peltoche/ical-rs` (Apache-2.0) read as a
//! reference for the parser shape (one generic component tree, typed views on
//! top). Clean-room.
//!
//! **Why Atlas wants it.** The business hub wants a client list and a shared
//! calendar that business partners — each on their own Atlas, or on no Atlas
//! at all — can exchange. Offline, with no service in the middle, the
//! exchange format is a file, and these are the files. A partner's Outlook
//! invite is an `.ics`; a client's business card is a `.vcf`. Without this,
//! either Atlas invents a format nobody else reads, or the data is re-typed.
//!
//! The typed views read what Atlas uses and keep everything else in the
//! generic tree, so a file read and written back loses nothing it did not
//! understand.

use crate::recur::{format_ical_time, parse_ical_time, Rule};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Prop {
    pub name: String,
    pub params: Vec<(String, String)>,
    pub value: String,
}

impl Prop {
    fn new(name: &str, value: &str) -> Prop {
        Prop { name: name.to_ascii_uppercase(), params: vec![], value: value.to_string() }
    }
    fn param(&self, k: &str) -> Option<&str> {
        self.params.iter().find(|(n, _)| n.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Component {
    pub name: String,
    pub props: Vec<Prop>,
    pub children: Vec<Component>,
}

impl Component {
    fn get(&self, name: &str) -> Option<&Prop> {
        self.props.iter().find(|p| p.name.eq_ignore_ascii_case(name))
    }
    fn all(&self, name: &str) -> Vec<&Prop> {
        self.props.iter().filter(|p| p.name.eq_ignore_ascii_case(name)).collect()
    }
}

/// Undo line folding: a line starting with a space or tab continues the one before.
pub fn unfold(text: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(last) = out.last_mut().filter(|_| line.starts_with(' ') || line.starts_with('\t')) {
            last.push_str(&line[1..]);
        } else if !line.is_empty() {
            out.push(line.to_string());
        }
    }
    out
}

/// Fold one logical line at 75 octets, never inside a UTF-8 character.
fn fold(line: &str) -> String {
    let mut out = String::new();
    let mut count = 0;
    for ch in line.chars() {
        let n = ch.len_utf8();
        if count + n > 75 {
            out.push_str("\r\n ");
            count = 1;
        }
        out.push(ch);
        count += n;
    }
    out.push_str("\r\n");
    out
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,").replace('\n', "\\n")
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some(o) => out.push(o),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn parse_line(line: &str) -> Result<Prop, String> {
    // The value starts at the first ':' not inside a quoted parameter.
    let mut in_q = false;
    let mut colon = None;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_q = !in_q,
            ':' if !in_q => {
                colon = Some(i);
                break;
            }
            _ => {}
        }
    }
    let colon = colon.ok_or_else(|| format!("no ':' in '{line}'"))?;
    let (head, value) = (&line[..colon], &line[colon + 1..]);
    let mut parts = head.split(';');
    let mut name = parts.next().unwrap_or("").to_ascii_uppercase();
    // vCard groups: "item1.EMAIL" → EMAIL
    if let Some((_, n)) = name.rsplit_once('.') {
        name = n.to_string();
    }
    if name.is_empty() {
        return Err(format!("no property name in '{line}'"));
    }
    let params = parts
        .filter_map(|p| p.split_once('=').map(|(k, v)| (k.to_ascii_uppercase(), v.trim_matches('"').to_string())))
        .collect();
    Ok(Prop { name, params, value: value.to_string() })
}

/// Parse every top-level component (VCARD, VCALENDAR) in a file.
pub fn parse(text: &str) -> Result<Vec<Component>, String> {
    let mut stack: Vec<Component> = vec![];
    let mut out = vec![];
    for line in unfold(text) {
        let p = parse_line(&line)?;
        match p.name.as_str() {
            "BEGIN" => stack.push(Component { name: p.value.to_ascii_uppercase(), ..Default::default() }),
            "END" => {
                let c = stack.pop().ok_or("END without BEGIN")?;
                if !c.name.eq_ignore_ascii_case(&p.value) {
                    return Err(format!("END:{} closes BEGIN:{}", p.value, c.name));
                }
                match stack.last_mut() {
                    Some(parent) => parent.children.push(c),
                    None => out.push(c),
                }
            }
            _ => stack.last_mut().ok_or_else(|| format!("'{line}' is outside any BEGIN"))?.props.push(p),
        }
    }
    if let Some(c) = stack.last() {
        return Err(format!("BEGIN:{} never ended", c.name));
    }
    Ok(out)
}

pub fn write(c: &Component) -> String {
    let mut s = fold(&format!("BEGIN:{}", c.name));
    for p in &c.props {
        let mut head = p.name.clone();
        for (k, v) in &p.params {
            let v = if v.contains([':', ';', ',']) { format!("\"{v}\"") } else { v.clone() };
            head.push_str(&format!(";{k}={v}"));
        }
        s.push_str(&fold(&format!("{head}:{}", p.value)));
    }
    for ch in &c.children {
        s.push_str(&write(ch));
    }
    s.push_str(&fold(&format!("END:{}", c.name)));
    s
}

// ---------- typed views ----------

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Card {
    pub name: String,
    pub emails: Vec<String>,
    pub phones: Vec<String>,
    pub org: String,
    pub note: String,
    pub uid: String,
}

impl Card {
    pub fn from_component(c: &Component) -> Option<Card> {
        if c.name != "VCARD" {
            return None;
        }
        let text = |n: &str| c.get(n).map(|p| unescape(&p.value)).unwrap_or_default();
        let mut name = text("FN");
        if name.is_empty() {
            // N:Family;Given;Additional;Prefix;Suffix
            if let Some(n) = c.get("N") {
                let f: Vec<&str> = n.value.split(';').collect();
                name = format!("{} {}", f.get(1).unwrap_or(&""), f.first().unwrap_or(&"")).trim().to_string();
            }
        }
        Some(Card {
            name,
            emails: c.all("EMAIL").iter().map(|p| p.value.trim().to_string()).collect(),
            phones: c.all("TEL").iter().map(|p| p.value.trim_start_matches("tel:").trim().to_string()).collect(),
            org: text("ORG").split(';').next().unwrap_or("").to_string(),
            note: text("NOTE"),
            uid: text("UID"),
        })
    }

    pub fn to_component(&self) -> Component {
        let mut props = vec![Prop::new("VERSION", "4.0"), Prop::new("FN", &escape(&self.name))];
        if !self.uid.is_empty() {
            props.push(Prop::new("UID", &self.uid));
        }
        for e in &self.emails {
            props.push(Prop::new("EMAIL", e));
        }
        for t in &self.phones {
            let mut p = Prop::new("TEL", &format!("tel:{t}"));
            p.params.push(("VALUE".into(), "uri".into()));
            props.push(p);
        }
        if !self.org.is_empty() {
            props.push(Prop::new("ORG", &escape(&self.org)));
        }
        if !self.note.is_empty() {
            props.push(Prop::new("NOTE", &escape(&self.note)));
        }
        Component { name: "VCARD".into(), props, children: vec![] }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub uid: String,
    pub summary: String,
    /// Local seconds (see `civil`); a `Z` time is read as-is — gap G1.
    pub start: i64,
    pub end: Option<i64>,
    pub all_day: bool,
    pub location: String,
    pub description: String,
    pub rule: Option<Rule>,
    pub exdates: Vec<i64>,
    /// The zone DTSTART's wall clock is in (`tz::Zone::id`), when it is not
    /// UTC — what a repeating event keeps across daylight-saving changes.
    pub zone: Option<String>,
}

impl Event {
    fn from_component(c: &Component, zones: &Zones) -> Result<Option<Event>, String> {
        if c.name != "VEVENT" {
            return Ok(None);
        }
        let text = |n: &str| c.get(n).map(|p| unescape(&p.value)).unwrap_or_default();
        // A timed value: `Z` is UTC; `TZID=` is that zone's wall clock; neither
        // ("floating") is the wall clock wherever you are — your zone.
        let timed = |p: &Prop, v: &str| -> Result<i64, String> {
            let t = parse_ical_time(v)?;
            if v.ends_with('Z') || v.ends_with('z') {
                return Ok(t);
            }
            Ok(zones.for_tzid(p.param("TZID")).to_utc(t))
        };
        let ds = c.get("DTSTART").ok_or("VEVENT without DTSTART")?;
        let all_day = ds.param("VALUE").map_or(ds.value.len() == 8, |v| v.eq_ignore_ascii_case("DATE"));
        let start = if all_day {
            parse_ical_time(&ds.value)? - (23 * 3600 + 59 * 60 + 59) // date-only → start of day
        } else {
            timed(ds, &ds.value)?
        };
        let end = match c.get("DTEND") {
            Some(p) if p.value.len() == 8 => Some(parse_ical_time(&p.value)? - (23 * 3600 + 59 * 60 + 59)),
            Some(p) => Some(timed(p, &p.value)?),
            None => None,
        };
        let rule = c.get("RRULE").map(|p| Rule::parse(&p.value)).transpose()?;
        let mut exdates = vec![];
        for p in c.all("EXDATE") {
            for v in p.value.split(',') {
                exdates.push(if v.len() == 8 { parse_ical_time(v)? - (23 * 3600 + 59 * 60 + 59) } else { timed(p, v)? });
            }
        }
        Ok(Some(Event {
            uid: text("UID"),
            summary: text("SUMMARY"),
            start,
            end,
            all_day,
            location: text("LOCATION"),
            description: text("DESCRIPTION"),
            rule,
            exdates,
            zone: {
                let z = if ds.value.ends_with('Z') || all_day { None } else { Some(zones.for_tzid(ds.param("TZID"))) };
                z.filter(|z| !z.is_utc()).map(|z| z.id())
            },
        }))
    }

    fn to_component(&self, stamp: i64) -> Component {
        // Atlas's calendar keeps UTC, so a timed event is written as UTC
        // (`Z`) — which lets Outlook, Google or a phone show it in their own
        // time zone correctly instead of guessing.
        //
        // A repeating event that keeps a wall clock in a named zone is the
        // exception: in UTC its RRULE would drift an hour at each clock change
        // on the other side too, so it goes out as `TZID=<IANA name>` local
        // time, which Google, Apple and Outlook all expand in that zone.
        let zone = self
            .zone
            .as_deref()
            .filter(|_| self.rule.is_some() && !self.all_day)
            .and_then(crate::tz::Zone::named)
            .filter(|z| z.name.contains('/'));
        let t = |x: i64| {
            if self.all_day {
                format_ical_time(x)[..8].to_string()
            } else if let Some(z) = &zone {
                format_ical_time(z.to_local(x))
            } else {
                format!("{}Z", format_ical_time(x))
            }
        };
        let tag = |p: &mut Prop| {
            if let Some(z) = &zone {
                p.params.push(("TZID".into(), z.name.clone()));
            }
        };
        let mut dt = Prop::new("DTSTART", &t(self.start));
        tag(&mut dt);
        if self.all_day {
            dt.params.push(("VALUE".into(), "DATE".into()));
        }
        let mut props = vec![Prop::new("UID", &self.uid), Prop::new("DTSTAMP", &format!("{}Z", format_ical_time(stamp))), dt];
        if let Some(e) = self.end {
            let mut p = Prop::new("DTEND", &t(e));
            tag(&mut p);
            if self.all_day {
                p.params.push(("VALUE".into(), "DATE".into()));
            }
            props.push(p);
        }
        props.push(Prop::new("SUMMARY", &escape(&self.summary)));
        if !self.location.is_empty() {
            props.push(Prop::new("LOCATION", &escape(&self.location)));
        }
        if !self.description.is_empty() {
            props.push(Prop::new("DESCRIPTION", &escape(&self.description)));
        }
        if let Some(r) = &self.rule {
            props.push(Prop::new("RRULE", &r.to_rrule()));
        }
        for x in &self.exdates {
            let mut p = Prop::new("EXDATE", &t(*x));
            tag(&mut p);
            props.push(p);
        }
        Component { name: "VEVENT".into(), props, children: vec![] }
    }
}

/// The zones an `.ics` file's times can be in: the file's own VTIMEZONE
/// definitions first (they are what the sender's calendar meant), then the
/// name tables in `tz`, then your zone for anything unnamed or unknown.
pub struct Zones {
    pub home: crate::tz::Zone,
    own: Vec<(String, crate::tz::Zone)>,
    /// TZIDs that matched nothing and were read as your zone.
    pub unknown: std::cell::RefCell<Vec<String>>,
}

impl Zones {
    fn for_tzid(&self, tzid: Option<&str>) -> crate::tz::Zone {
        let Some(id) = tzid else { return self.home.clone() };
        if let Some((_, z)) = self.own.iter().find(|(n, _)| n == id) {
            return z.clone();
        }
        if let Some(z) = crate::tz::Zone::named(id) {
            return z;
        }
        let mut u = self.unknown.borrow_mut();
        if !u.iter().any(|x| x == id) {
            u.push(id.to_string());
        }
        self.home.clone()
    }
}

/// A VTIMEZONE as a POSIX rule: STANDARD and DAYLIGHT each give an offset,
/// and (when the zone changes clocks) a yearly RRULE `BYMONTH=m;BYDAY=nDD`
/// and the local time of the change in DTSTART.
fn zone_from_vtimezone(c: &Component) -> Option<(String, crate::tz::Zone)> {
    let id = c.get("TZID")?.value.clone();
    let off = |s: &str| -> Option<i64> {
        let (sign, d) = match s.as_bytes().first()? {
            b'-' => (-1, &s[1..]),
            b'+' => (1, &s[1..]),
            _ => (1, s),
        };
        let h: i64 = d.get(0..2)?.parse().ok()?;
        let m: i64 = d.get(2..4).and_then(|x| x.parse().ok()).unwrap_or(0);
        Some(sign * (h * 3600 + m * 60))
    };
    let posix_off = |secs: i64| -> String {
        let w = -secs;
        let (sign, a) = if w < 0 { ("-", -w) } else { ("", w) };
        format!("{sign}{}:{:02}", a / 3600, a % 3600 / 60)
    };
    let part = |name: &str| c.children.iter().find(|k| k.name == name);
    let rule_of = |k: &Component| -> Option<String> {
        let r = &k.get("RRULE")?.value;
        let field = |f: &str| r.split(';').find_map(|kv| kv.strip_prefix(f)).map(|v| v.to_string());
        let month: u32 = field("BYMONTH=")?.parse().ok()?;
        let byday = field("BYDAY=")?;
        // Checked: one letter underflowed, and a non-ASCII byte split a
        // character (fuzzed, 5 Oct 2026).
        let cut = byday.len().checked_sub(2).filter(|&c| byday.is_char_boundary(c))?;
        let (n, dd) = byday.split_at(cut);
        let week: i32 = if n.is_empty() { 1 } else { n.parse().ok()? };
        let week = if week < 0 { 5 } else { week.clamp(1, 5) };
        let day = ["SU", "MO", "TU", "WE", "TH", "FR", "SA"].iter().position(|d| *d == dd)?;
        let ds = &k.get("DTSTART")?.value;
        let hh: u32 = ds.get(9..11)?.parse().ok()?;
        let mm: u32 = ds.get(11..13)?.parse().ok()?;
        Some(format!("M{month}.{week}.{day}/{hh}:{mm:02}"))
    };
    // The abbreviation the file gives (TZNAME), else the offset as "UTC-8".
    let abbr = |k: &Component, secs: i64| -> String {
        let named: String = k.get("TZNAME").map(|p| p.value.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '+' || *c == '-').collect()).unwrap_or_default();
        if named.len() >= 3 {
            named
        } else if secs % 3600 == 0 {
            format!("UTC{:+}", secs / 3600)
        } else {
            format!("UTC{:+}{:02}", secs / 3600, (secs.abs() % 3600) / 60)
        }
    };
    let std = part("STANDARD")?;
    let std_off = off(&std.get("TZOFFSETTO")?.value)?;
    let spec = match part("DAYLIGHT") {
        Some(dl) => {
            let dst_off = off(&dl.get("TZOFFSETTO")?.value)?;
            format!("<{}>{}<{}>{},{},{}", abbr(std, std_off), posix_off(std_off), abbr(dl, dst_off), posix_off(dst_off), rule_of(dl)?, rule_of(std)?)
        }
        None => format!("<{}>{}", abbr(std, std_off), posix_off(std_off)),
    };
    let mut z = crate::tz::Zone::posix(&spec)?;
    z.name = id.clone();
    Some((id, z))
}

/// Every event in an `.ics` file, times converted to UTC. `home` is your
/// zone: floating times and unknown TZIDs are read in it (and the unknown
/// ones listed in the returned `Zones`, so the caller can say so).
pub fn events_in(text: &str, home: &crate::tz::Zone) -> Result<(Vec<Event>, Zones), String> {
    let cals = parse(text)?;
    let own = cals.iter().flat_map(|cal| cal.children.iter()).filter(|c| c.name == "VTIMEZONE").filter_map(zone_from_vtimezone).collect();
    let zones = Zones { home: home.clone(), own, unknown: Default::default() };
    let mut out = vec![];
    for cal in &cals {
        for c in &cal.children {
            if let Some(e) = Event::from_component(c, &zones)? {
                out.push(e);
            }
        }
    }
    Ok((out, zones))
}

/// Wrap events in a VCALENDAR.
pub fn calendar(events: &[Event], stamp: i64) -> String {
    let cal = Component {
        name: "VCALENDAR".into(),
        props: vec![Prop::new("VERSION", "2.0"), Prop::new("PRODID", "-//Atlas//Atlas//EN")],
        children: events.iter().map(|e| e.to_component(stamp)).collect(),
    };
    write(&cal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::civil::Civil;

    const OUTLOOK: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Microsoft Corporation//Outlook 16.0//EN\r\nBEGIN:VEVENT\r\nUID:040000008200E00074C5B7101A82E0080000000\r\n 0A1B2C3D\r\nDTSTART;TZID=Pacific Standard Time:20260929T100000\r\nDTEND;TZID=Pacific Standard Time:20260929T103000\r\nRRULE:FREQ=WEEKLY;BYDAY=TU;COUNT=6\r\nSUMMARY:Weekly sync\\, ops\r\nLOCATION:Room 4\\; east wing\r\nDESCRIPTION:Agenda:\\n1. installer\\n2. shadow on F\r\nX-MICROSOFT-CDO-BUSYSTATUS:BUSY\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn reads_an_outlook_invite() {
        let ev = events_in(OUTLOOK, &crate::tz::Zone::utc()).unwrap().0;
        assert_eq!(ev.len(), 1);
        let e = &ev[0];
        assert_eq!(e.uid, "040000008200E00074C5B7101A82E00800000000A1B2C3D"); // unfolded
        assert_eq!(e.summary, "Weekly sync, ops");
        assert_eq!(e.location, "Room 4; east wing");
        assert!(e.description.contains("\n2. shadow on F"));
        // 10:00 Pacific Standard Time (daylight saving in September) is 17:00 UTC.
        let c = Civil::from_local(e.start);
        assert_eq!((c.month, c.day, c.hour), (9, 29, 17));
        let occ = crate::recur::Series::new(e.start, e.rule.clone().unwrap()).between(i64::MIN, i64::MAX, 100);
        assert_eq!(occ.len(), 6);
    }

    #[test]
    fn unknown_properties_survive_a_round_trip() {
        let comps = parse(OUTLOOK).unwrap();
        let again = parse(&write(&comps[0])).unwrap();
        assert_eq!(comps, again);
        assert!(write(&comps[0]).contains("X-MICROSOFT-CDO-BUSYSTATUS:BUSY"));
    }

    #[test]
    fn folding_is_75_octets_and_utf8_safe() {
        let long = format!("DESCRIPTION:{}", "é".repeat(100));
        let f = fold(&long);
        for l in f.split("\r\n").filter(|l| !l.is_empty()) {
            assert!(l.len() <= 75, "{} octets", l.len());
        }
        assert_eq!(unfold(&f), vec![long]);
    }

    #[test]
    fn vcard_both_versions() {
        let v3 = "BEGIN:VCARD\nVERSION:3.0\nN:Smith;John;;;\nitem1.EMAIL;TYPE=INTERNET:john@acme.com\nTEL;TYPE=CELL:+1 555 010 2000\nORG:Acme Corp;Sales\nEND:VCARD\n";
        let cards: Vec<Card> = parse(v3).unwrap().iter().filter_map(Card::from_component).collect();
        assert_eq!(cards[0].name, "John Smith");
        assert_eq!(cards[0].emails, vec!["john@acme.com"]);
        assert_eq!(cards[0].org, "Acme Corp");
        let back = write(&cards[0].to_component());
        let again = Card::from_component(&parse(&back).unwrap()[0]).unwrap();
        assert_eq!(again, cards[0]);
    }

    #[test]
    fn written_calendar_reads_back() {
        let e = Event {
            uid: "atlas-1".into(),
            summary: "Dentist; bring card".into(),
            start: Civil { year: 2026, month: 10, day: 2, hour: 0, minute: 0, second: 0 }.to_local(),
            end: None,
            all_day: true,
            location: String::new(),
            description: String::new(),
            rule: None,
            exdates: vec![],
            zone: None,
        };
        let s = calendar(&[e.clone()], 0);
        assert!(s.contains("DTSTART;VALUE=DATE:20261002"));
        assert_eq!(events_in(&s, &crate::tz::Zone::utc()).unwrap().0, vec![e]);
    }

    #[test]
    fn malformed_files_are_errors() {
        assert!(parse("BEGIN:VCARD\nFN:x\n").is_err());
        assert!(parse("FN:x\n").is_err());
        assert!(parse("BEGIN:VCARD\nEND:VEVENT\n").is_err());
        assert!(events_in("BEGIN:VCALENDAR\nBEGIN:VEVENT\nSUMMARY:x\nEND:VEVENT\nEND:VCALENDAR\n", &crate::tz::Zone::utc()).is_err());
    }
}
