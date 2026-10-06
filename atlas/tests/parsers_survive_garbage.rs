//! Everything Atlas reads from outside -- an invite, a zip, a PDF, a mail
//! server's reply, a 3D model, an update package -- survives garbage
//! (5 Oct 2026 audit, Q20: "no fuzzing of the iCalendar/IMAP/export
//! parsers").
//!
//! Not a fuzzer with coverage feedback (cargo-fuzz needs nightly); a
//! deterministic one that runs in every `cargo test`: for each parser,
//! thousands of inputs made by corrupting a valid seed (flipped bytes,
//! bytes inserted and cut, the file cut short, a chunk repeated) plus pure
//! noise. A parser may refuse any of them; none may panic, and none may take
//! more than a moment. A panic here is a crash Atlas would have had on a
//! file someone sent you. Same seed every run, so a failure reproduces.

use std::io::{Read, Write};
use std::time::{Duration, Instant};

/// xorshift64*: tiny, deterministic, good enough to make garbage.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
}

/// Letters whose lowercase is a different number of bytes (Turkish İ grows,
/// the German capital sharp s, the Kelvin and Ohm signs shrink), and some
/// ordinary multi-byte ones. Code that finds a phrase in `to_lowercase()`
/// and cuts the original there lands past the end or mid-character.
const SHIFTING: [&str; 7] = ["\u{130}", "\u{1E9E}", "\u{212A}", "\u{2126}", "\u{E9}", "\u{2615}", "\u{FFFD}"];

/// `seed` with a few of those letters put in at character boundaries.
fn with_shifting_letters(rng: &mut Rng, seed: &str) -> String {
    let mut s = seed.to_string();
    for _ in 0..1 + rng.below(4) {
        let cuts: Vec<usize> = s.char_indices().map(|(i, _)| i).chain([s.len()]).collect();
        let at = cuts[rng.below(cuts.len())];
        s.insert_str(at, SHIFTING[rng.below(SHIFTING.len())]);
    }
    s
}

/// One corrupted copy of `seed`, or noise.
fn mutate(rng: &mut Rng, seed: &[u8]) -> Vec<u8> {
    if rng.below(4) == 0 {
        return with_shifting_letters(rng, &String::from_utf8_lossy(seed)).into_bytes();
    }
    if rng.below(8) == 0 {
        let n = rng.below(600);
        return (0..n).map(|_| rng.next() as u8).collect();
    }
    let mut v = seed.to_vec();
    for _ in 0..1 + rng.below(6) {
        match rng.below(6) {
            0 if !v.is_empty() => {
                let i = rng.below(v.len());
                v[i] ^= 1 << rng.below(8);
            }
            1 if !v.is_empty() => {
                let i = rng.below(v.len());
                v[i] = rng.next() as u8;
            }
            2 => {
                let i = rng.below(v.len() + 1);
                let b = [b'0', b'9', b'-', b':', b';', b'\n', b'{', b'}', 0xFF, 0x00, b'"'][rng.below(11)];
                v.insert(i, b);
            }
            3 if !v.is_empty() => {
                let i = rng.below(v.len());
                let n = 1 + rng.below(16.min(v.len() - i));
                v.drain(i..i + n);
            }
            4 => {
                let keep = rng.below(v.len() + 1);
                v.truncate(keep);
            }
            _ if v.len() > 4 => {
                let i = rng.below(v.len() - 4);
                let n = 1 + rng.below(32.min(v.len() - i));
                let chunk = v[i..i + n].to_vec();
                let at = rng.below(v.len());
                v.splice(at..at, chunk);
            }
            _ => {}
        }
    }
    v
}

/// Run `parse` on `rounds` corruptions of each seed. Fails naming the first
/// input that panicked (hex, so it can be pasted into a test) or ran long.
fn survive(name: &str, seeds: &[&[u8]], rounds: usize, parse: impl Fn(&[u8]) + std::panic::RefUnwindSafe) -> usize {
    // ATLAS_FUZZ_SEED / ATLAS_FUZZ_TIMES for a longer hunt by hand; every
    // ordinary run is the same few thousand inputs, so a failure reproduces.
    let seed: u64 = std::env::var("ATLAS_FUZZ_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(0x9E37_79B9_7F4A_7C15);
    let rounds = rounds * std::env::var("ATLAS_FUZZ_TIMES").ok().and_then(|v| v.parse().ok()).unwrap_or(1usize);
    let mut rng = Rng(seed ^ name.len() as u64);
    let quiet = std::panic::take_hook();
    // Kept, not printed: where it panicked and what it said.
    static WHERE: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
    std::panic::set_hook(Box::new(|info| {
        let at = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let said = info.payload().downcast_ref::<String>().cloned().or_else(|| info.payload().downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
        // A panic inside the standard library (slicing, unwrap) is named by
        // the first line of Atlas's own code that led to it.
        let ours = if at.starts_with("src/") {
            String::new()
        } else {
            let bt = std::backtrace::Backtrace::force_capture().to_string();
            bt.lines()
                .map(str::trim)
                .find(|l| l.starts_with("at ") && (l.contains("src/") || l.contains("src\\")) && !l.contains("/rustc/") && !l.contains("library/") && !l.contains(".cargo") && !l.contains("tests/"))
                .map(|l| format!(" (from {l})"))
                .unwrap_or_default()
        };
        *WHERE.lock().unwrap_or_else(|p| p.into_inner()) = format!("{at}: {said}{ours}");
    }));
    let mut failure = None;
    let mut tried = 0;
    'all: for seed in seeds {
        for _ in 0..rounds {
            let input = mutate(&mut rng, seed);
            tried += 1;
            let started = Instant::now();
            let r = std::panic::catch_unwind(|| parse(&input));
            let took = started.elapsed();
            if r.is_err() {
                let at = WHERE.lock().unwrap_or_else(|p| p.into_inner()).clone();
                failure = Some(format!("{name} panicked at {at}\non {} bytes: {}", input.len(), hex(&input)));
                break 'all;
            }
            if took > Duration::from_secs(2) {
                failure = Some(format!("{name} took {took:?} on {} bytes: {}", input.len(), hex(&input)));
                break 'all;
            }
        }
    }
    std::panic::set_hook(quiet);
    if let Some(f) = failure {
        panic!("{f}");
    }
    tried
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

const ICS: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTIMEZONE\r\nTZID:Custom Pacific\r\nBEGIN:STANDARD\r\nDTSTART:19701101T020000\r\nRRULE:FREQ=YEARLY;BYDAY=1SU;BYMONTH=11\r\nTZOFFSETFROM:-0700\r\nTZOFFSETTO:-0800\r\nEND:STANDARD\r\nBEGIN:DAYLIGHT\r\nDTSTART:19700308T020000\r\nRRULE:FREQ=YEARLY;BYDAY=2SU;BYMONTH=3\r\nTZOFFSETFROM:-0800\r\nTZOFFSETTO:-0700\r\nEND:DAYLIGHT\r\nEND:VTIMEZONE\r\nBEGIN:VEVENT\r\nUID:a1\r\nDTSTART;TZID=Custom Pacific:20260929T100000\r\nDTEND;TZID=Custom Pacific:20260929T103000\r\nRRULE:FREQ=WEEKLY;BYDAY=TU,TH;INTERVAL=2;COUNT=8\r\nEXDATE:20261006T170000Z\r\nSUMMARY:Weekly sync\\, with notes\r\n folded on\r\nEND:VEVENT\r\nBEGIN:VCARD\r\nFN:Jo Bloggs\r\nEMAIL:jo@example.com\r\nEND:VCARD\r\nEND:VCALENDAR\r\n";

#[test]
fn calendar_invites_and_cards() {
    let tried = survive("vformat::parse", &[ICS.as_bytes()], 3000, |b| {
        if let Ok(cs) = atlas::vformat::parse(&text(b)) {
            for c in &cs {
                let _ = atlas::vformat::Card::from_component(c);
            }
        }
    });
    assert!(tried > 0, "nothing was tried");
    let home = atlas::tz::Zone::named("America/New_York").unwrap();
    let tried = survive("calendar::import_ics", &[ICS.as_bytes()], 1500, |b| {
        let mut cal = atlas::calendar::Calendar::default();
        if cal.import_ics(&text(b), 0, &home).is_ok() {
            let _ = cal.occurrences_between(1_790_000_000, 1_800_000_000);
        }
    });
    assert!(tried > 0, "nothing was tried");
}

#[test]
fn repeat_rules_times_and_zones() {
    let rules: [&[u8]; 3] = [b"FREQ=WEEKLY;BYDAY=MO,WE,-1FR;INTERVAL=2;COUNT=5", b"FREQ=MONTHLY;BYMONTHDAY=31,-1;UNTIL=20271231T000000Z", b"FREQ=YEARLY;BYMONTH=2;BYDAY=4TH;BYSETPOS=-1"];
    survive("recur::Rule::parse", &rules, 3000, |b| {
        if let Ok(rule) = atlas::recur::Rule::parse(&text(b)) {
            let s = atlas::recur::Series::new(1_790_000_000, rule);
            let _ = s.between(1_790_000_000, 1_800_000_000, 200);
        }
    });
    let tried = survive("recur::parse_ical_time", &[b"20260929T100000Z", b"20260229"], 3000, |b| {
        let _ = atlas::recur::parse_ical_time(&text(b));
    });
    assert!(tried > 0, "nothing was tried");
    let tried = survive("tz::Zone::posix", &[b"CET-1CEST,M3.5.0,M10.5.0/3", b"<+0530>-5:30", b"PST8PDT,J60/2,300"], 3000, |b| {
        if let Some(z) = atlas::tz::Zone::posix(&text(b)) {
            for t in [0i64, 1_790_000_000, -86_400 * 365 * 3000, i64::MAX / 4] {
                let _ = z.offset_at(t);
                let _ = z.to_utc(t);
            }
        }
    });
    assert!(tried > 0, "nothing was tried");
}

#[test]
fn http_replies() {
    let chunked: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
    let plain: &[u8] = b"HTTP/1.1 301 Moved\r\nLocation: https://example.com/\r\nContent-Length: 4\r\n\r\nbody";
    let tried = survive("http::parse_response", &[chunked, plain], 4000, |b| {
        let _ = atlas::http::parse_response(b);
        let _ = atlas::http::whole_reply(b);
    });
    assert!(tried > 0, "nothing was tried");
}

const ZIP: &str = "504b03041400000008009783455d074e6a2b12000000250000000b0000006e6f7465732f612e747874cb48cdc9c95728cf2fca49d151c8c0c10100504b03041400000008009783455d1542be650f0000000d00000006000000622e6a736f6eab56ca56b28a36d431d2318ead0500504b010214031400000008009783455d074e6a2b12000000250000000b00000000000000000000008001000000006e6f7465732f612e747874504b010214031400000008009783455d1542be650f0000000d00000006000000000000000000000080013b000000622e6a736f6e504b050600000000020002006d0000006e0000000000";

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

#[test]
fn zips_and_update_packages() {
    let zip = unhex(ZIP);
    let tried = survive("unpack::entries_of/read_entry", &[&zip], 4000, |b| {
        if let Ok(es) = atlas::unpack::entries_of(b) {
            for e in &es {
                let _ = atlas::unpack::read_entry(b, e);
            }
        }
    });
    assert!(tried > 0, "nothing was tried");
    let tried = survive("zipread::file_inside", &[&zip], 3000, |b| {
        let _ = atlas::zipread::file_inside(b, |n| n.ends_with(".json"), 1 << 20);
    });
    assert!(tried > 0, "nothing was tried");
    let tried = survive("ota::Package::read", &[&zip], 3000, |b| {
        let _ = atlas::ota::Package::read(b);
    });
    assert!(tried > 0, "nothing was tried");
}

const PDF: &str = "%PDF-1.4\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >> endobj\n4 0 obj << /Length 44 >> stream\nBT /F1 12 Tf 20 100 Td (Hello, invoice 42) Tj ET\nendstream endobj\n5 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> endobj\nxref\n0 6\n0000000000 65535 f \ntrailer << /Size 6 /Root 1 0 R >>\nstartxref\n0\n%%EOF\n";

/// A compressed stream, so the `/Filter` reading is exercised too (it
/// panicked when `/Filter` ended the dictionary).
fn pdf_with_filter() -> Vec<u8> {
    let body = miniz_oxide::deflate::compress_to_vec_zlib(b"BT /F1 12 Tf 20 100 Td (Compressed hello) Tj ET", 6);
    let mut pdf = format!("%PDF-1.4\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n3 0 obj << /Type /Page /Parent 2 0 R /Contents 4 0 R >> endobj\n4 0 obj << /Length {} /Filter /FlateDecode >> stream\n", body.len()).into_bytes();
    pdf.extend_from_slice(&body);
    pdf.extend_from_slice(b"\nendstream endobj\ntrailer << /Root 1 0 R >>\n%%EOF\n");
    pdf
}

#[test]
fn pdfs() {
    let filtered = pdf_with_filter();
    let tried = survive("pdftext::read", &[PDF.as_bytes(), &filtered], 3000, |b| {
        let _ = atlas::pdftext::read(b);
    });
    assert!(tried > 0, "nothing was tried");
}

const STL: &str = "solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid t\n";
const OBJ: &str = "v 0 0 0\nv 1 0 0\nv 0 1 0\nvt 0 0\nvn 0 0 1\nf 1/1/1 2/1/1 3/1/1\nf -1 -2 -3\n";
const GLTF: &str = r#"{"asset":{"version":"2.0"},"buffers":[{"byteLength":36,"uri":"data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAACAPwAAAAAAAAAAAAAAAAAAgD8AAAAA"}],"bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"}],"meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}]}"#;

#[test]
fn three_d_models() {
    let tried = survive("meshio::read_stl", &[STL.as_bytes()], 3000, |b| {
        let _ = atlas::meshio::read_stl(b);
    });
    assert!(tried > 0, "nothing was tried");
    let tried = survive("meshio::read_obj", &[OBJ.as_bytes()], 3000, |b| {
        let _ = atlas::meshio::read_obj(&text(b), None);
    });
    assert!(tried > 0, "nothing was tried");
    let tried = survive("meshio::read_gltf", &[GLTF.as_bytes()], 3000, |b| {
        let _ = atlas::meshio::read_gltf(b, None);
    });
    assert!(tried > 0, "nothing was tried");
}

/// A mail server that says whatever the bytes say, then hangs up.
struct Server {
    said: std::io::Cursor<Vec<u8>>,
}
impl Read for Server {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.said.read(buf)
    }
}
impl Write for Server {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_mail_servers_replies() {
    let fetch: &[u8] = b"* OK ready\r\n* 1 FETCH (UID 7 FLAGS (\\Seen) BODY[HEADER.FIELDS (FROM SUBJECT DATE)] {58}\r\nFrom: Jo <jo@example.com>\r\nSubject: =?UTF-8?B?aGk=?=\r\n\r\n BODY[TEXT] {11}\r\nhello there)\r\nA1 OK done\r\nA2 OK done\r\n";
    let search: &[u8] = b"* OK ready\r\n* SEARCH 1 2 3 99\r\nA1 OK done\r\n* LIST (\\HasNoChildren \\Sent) \"/\" \"Sent Items\"\r\nA2 OK\r\n";
    let tried = survive("imap::Session (fetch, search, list)", &[fetch, search], 2500, |b| {
        let mut s = atlas::imap::Session::new(Server { said: std::io::Cursor::new(b.to_vec()) });
        let _ = s.read_greeting();
        let _ = s.uid_fetch(7);
        let _ = s.uid_search("ALL");
        let _ = s.sent_mailbox();
    });
    assert!(tried > 0, "nothing was tried");
}

// ---- what you say, with letters that change length when lowercased --------

#[test]
fn commands_with_letters_that_change_length() {
    // 5 Oct 2026: about twenty command readers found a phrase in a
    // `to_lowercase()` copy and cut the original there, so "read me the email
    // from İlker" ran past the end and panicked. They now lowercase ASCII only.
    let failed: std::cell::RefCell<Vec<String>> = std::cell::RefCell::new(Vec::new());
    let said = |seeds: &[&str], f: &dyn Fn(&str)| {
        let mut rng = Rng(0xA71A5);
        for seed in seeds {
            for _ in 0..400 {
                let s = with_shifting_letters(&mut rng, seed);
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&s))).is_err() {
                    failed.borrow_mut().push(s);
                    return;
                }
            }
        }
    };
    let quiet = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    said(&["send a text to Ilker saying I'm late", "text message Ana: on my way"], &|s| {
        let _ = atlas::texting::text_asked(s);
    });
    said(&["my goal is to run 5k", "set a goal to read more"], &|s| {
        let _ = atlas::nudge::goal_words(s);
    });
    said(&["no, I wanted the blue one instead", "it should have said Tuesday"], &|s| {
        let _ = atlas::revise::wanted_in(s);
    });
    said(&["add milk to the list for the family group as a reader"], &|s| {
        let _ = atlas::groups::read_spoken(s);
    });
    said(&["translate into Spanish: good morning", "what does this say in French"], &|s| {
        let _ = atlas::translation::read(s);
    });
    said(&["click the Send button in Outlook", "press the OK button"], &|s| {
        let _ = atlas::uia::button_request(s);
    });
    said(&["Atlas, no, I meant the other one", "nope, I meant tomorrow"], &|s| {
        let _ = atlas::phrasebook::meant_instead(s);
    });
    said(&["in Excel, add a total row", "open the budget sheet in Excel"], &|s| {
        let _ = atlas::operate::app_and_goal(s, &["Excel".to_string(), "Outlook".to_string()]);
    });
    said(&["what is the weather and who won the game?"], &|s| {
        let _ = atlas::asking::prepare(s);
    });
    let cfg = atlas::social::SocialConfig::default();
    said(&["watch youtube.com/channel/UCabcdefghijklmnopqrstuv", "watch #rustlang on mastodon"], &|s| {
        let _ = atlas::social::watchlist::parse_target(s, &cfg);
    });
    std::panic::set_hook(quiet);
    let failed = failed.into_inner();
    assert!(failed.is_empty(), "panicked on: {failed:#?}");
}

#[test]
fn no_position_is_taken_from_a_unicode_lowercased_copy() {
    // `x.to_lowercase().find(..)` gives a position in a copy whose length can
    // differ from `x`'s; cutting `x` there panics or cuts the wrong place.
    // `to_ascii_lowercase` keeps every byte where it was.
    let mut found = Vec::new();
    for (f, text) in crate::common::source_file_set() {
        let prod = text.split("#[cfg(test)]").next().unwrap_or("");
        for (n, line) in prod.lines().enumerate() {
            for shape in [".to_lowercase().find(", ".to_lowercase().rfind(", ".to_lowercase().match_indices(", ".to_lowercase().char_indices("] {
                if line.contains(shape) && !line.trim_start().starts_with("//") {
                    found.push(format!("src/{f}.rs:{}: {}", n + 1, line.trim()));
                }
            }
        }
    }
    assert!(found.is_empty(), "use to_ascii_lowercase when the position is used on the original:\n{}", found.join("\n"));
}
