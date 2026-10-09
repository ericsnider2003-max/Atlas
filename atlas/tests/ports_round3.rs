//! Round-3 ports, end to end. Each test drives the port through the part of
//! Atlas that uses it — the vault, the mail check, the calendar import, the
//! secondary model — not the module on its own.

use atlas::guessable;

#[test]
fn guessable_scores_patterns_not_character_classes() {
    let cases: &[(&str, u8, u8)] = &[
        ("password1234", 0, 1),
        ("aaaaaaaaaaaa", 0, 1),
        ("qwertyuiopasdf", 0, 1),
        ("123456789012", 0, 1),
        ("P@ssw0rd2024", 0, 2),
        ("iloveyou19901990", 0, 2),
        ("correct horse battery staple", 3, 4),
        ("the lamp my sister broke in june", 4, 4),
        ("xk3Tq9vLz2Rw", 3, 4),
    ];
    for (p, lo, hi) in cases {
        let e = guessable::estimate(p, &[]);
        println!("{p:<34} score {} 10^{:.1}  {}", e.score, e.guesses_log10, e.say());
        assert!((*lo..=*hi).contains(&e.score), "{p}: {}", e.score);
    }
    // Words about the person count against them.
    let plain = guessable::estimate("ericsnider2003", &[]);
    let known = guessable::estimate("ericsnider2003", &["eric", "snider"]);
    assert!(known.guesses_log10 <= plain.guesses_log10);
    let w = guessable::estimate("password1234", &[]).say();
    assert!(w.contains("\"password\" is on the list of common passwords"), "{w}");
}

// ---- tz ---------------------------------------------------------------------

fn utc(y: i64, m: u32, d: u32, h: u32, mi: u32) -> i64 {
    atlas::civil::Civil { year: y, month: m, day: d, hour: h, minute: mi, second: 0 }.to_local()
}

#[test]
fn tz_offsets_and_the_two_awkward_hours() {
    use atlas::tz::Zone;
    let la = Zone::named("America/Los_Angeles").unwrap();
    let win = Zone::named("Pacific Standard Time").unwrap();
    assert_eq!(la, win, "the Windows name is the same zone");
    // 2026: DST from Sun 8 Mar 10:00Z to Sun 1 Nov 09:00Z.
    assert_eq!(la.offset_at(utc(2026, 3, 8, 9, 59)), -8 * 3600);
    assert_eq!(la.offset_at(utc(2026, 3, 8, 10, 0)), -7 * 3600);
    assert_eq!(la.offset_at(utc(2026, 11, 1, 8, 59)), -7 * 3600);
    assert_eq!(la.offset_at(utc(2026, 11, 1, 9, 0)), -8 * 3600);
    assert_eq!(la.abbreviation_at(utc(2026, 7, 1, 0, 0)), "PDT");
    // 01:30 on 1 Nov happens twice: the first (PDT) is meant.
    assert_eq!(la.to_utc(utc(2026, 11, 1, 1, 30)), utc(2026, 11, 1, 8, 30));
    // 02:30 on 8 Mar never happens: read with the offset from before the gap.
    assert_eq!(la.to_utc(utc(2026, 3, 8, 2, 30)), utc(2026, 3, 8, 10, 30));
    // Southern hemisphere, and last-Sunday rules.
    let syd = Zone::named("AUS Eastern Standard Time").unwrap();
    assert_eq!(syd.offset_at(utc(2026, 1, 15, 0, 0)), 11 * 3600);
    assert_eq!(syd.offset_at(utc(2026, 7, 15, 0, 0)), 10 * 3600);
    let lon = Zone::named("Europe/London").unwrap();
    assert_eq!(lon.offset_at(utc(2026, 3, 29, 0, 59)), 0);
    assert_eq!(lon.offset_at(utc(2026, 3, 29, 1, 0)), 3600);
    assert_eq!(lon.offset_at(utc(2026, 10, 25, 0, 59)), 3600);
    assert_eq!(lon.offset_at(utc(2026, 10, 25, 1, 0)), 0);
    // Half-hours and fixed zones.
    assert_eq!(Zone::named("India Standard Time").unwrap().offset_at(0), 5 * 3600 + 1800);
    assert_eq!(Zone::named("<+0545>-5:45").unwrap().offset_at(0), 5 * 3600 + 2700);
    assert_eq!(Zone::named("(UTC-08:00) Pacific Time (US & Canada)").unwrap().name, "America/Los_Angeles");
    assert!(Zone::named("Not/AZone").is_none());
    // Every zone in the table parses.
    for n in ["America/Santiago", "Europe/Dublin", "Africa/Cairo", "Pacific/Auckland", "America/St_Johns"] {
        let z = Zone::named(n).unwrap_or_else(|| panic!("{n}"));
        let a = z.offset_at(utc(2026, 1, 15, 12, 0));
        let b = z.offset_at(utc(2026, 7, 15, 12, 0));
        assert_ne!(a, b, "{n} changes clocks");
    }
}

// ---- tz through the scheduler, the calendar, the daemon ----------------------

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::intent::Intent;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-ports-r3-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cfg_in(zone: &str) -> Config {
    let mut c = Config::load(Path::new("config")).unwrap();
    c.tools.as_mut().unwrap().time_zone = zone.into();
    c
}

#[test]
fn calendar_cli_refuses_unknown_export_and_does_not_claim_a_failed_import() {
    let root = tmp("calendar-cli-truth");
    let run = |args: &[&str]| {
        let result = std::process::Command::new(env!("CARGO_BIN_EXE_atlas"))
            .args(args).current_dir(&root).env("ATLAS_HOME", &root)
            .env("ATLAS_UPDATE_PROBE", "1").output().unwrap();
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
        String::from_utf8_lossy(&result.stdout).into_owned()
    };
    run(&["calendar"]); // first-run setup belongs to this disposable install
    let store = Store::new(root.join("data").join("state"));
    let corrupt = store.root().join("calendar.json");
    std::fs::write(&corrupt, b"{broken").unwrap();
    let destination = root.join("existing.ics");
    std::fs::write(&destination, b"keep this existing export").unwrap();
    let said = run(&["calendar", "export", destination.to_str().unwrap()]);
    assert!(said.contains("calendar is unavailable") && !said.contains("Wrote 0 events"), "{said}");
    assert_eq!(std::fs::read(&destination).unwrap(), b"keep this existing export");
    std::fs::remove_file(&corrupt).unwrap();
    let source = root.join("incoming.ics");
    std::fs::write(&source, "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:cli-synthetic\r\nDTSTART:20261009T120000Z\r\nDTEND:20261009T130000Z\r\nSUMMARY:Synthetic calendar import\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n").unwrap();
    let lock = store.transaction().unwrap();
    let said = run(&["calendar", "import", source.to_str().unwrap()]);
    assert!(said.contains("import is not confirmed") && !said.contains("event added or updated"), "{said}");
    drop(lock);
    assert!(atlas::calendar::Calendar::load(&store).is_empty(), "failed import must not be persisted");
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, root: PathBuf) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(root), Proactive::new(ProactiveConfig::default()))
}

fn show(port: &str, input: &str, output: &str) {
    println!("LIVE [{port}]\n  in:  {input}\n  out: {output}\n");
}

#[test]
fn tz_a_weekday_reminder_keeps_seven_on_your_clock_through_the_change() {
    let c = cfg_in("America/Los_Angeles");
    let p = plat();
    let mut d = daemon(&c, &p, tmp("remind"));
    let la = atlas::tz::Zone::named("America/Los_Angeles").unwrap();
    let said = "remind me every weekday at 7 to check the VPS";
    let out = d.turn(said, atlas::store::now());
    show("tz+cronspec+scheduler", said, &out);
    let job = d.scheduler.jobs.last().expect("a job").clone();
    println!("  job.on = {:?}, first due = {}", job.on, atlas::digest::iso_utc(job.due));
    assert_eq!(job.on.as_deref(), Some("cron[America/Los_Angeles]:0 7 * * MON-FRI"));
    assert_eq!(la.hour(job.due as i64), 7, "07:00 on the Los Angeles clock");
    // Across the November change the UTC hour moves and the local one doesn't.
    let fri_oct30 = utc(2026, 10, 30, 14, 0) as u64; // 07:00 PDT
    d.scheduler.complete(job.id, fri_oct30, "done", true);
    let next = d.scheduler.jobs.iter().find(|j| j.id == job.id).unwrap().due;
    println!("  ran Fri 30 Oct 07:00 PDT -> next {} ({} local)", atlas::digest::iso_utc(next), la.hour(next as i64));
    assert_eq!(next, utc(2026, 11, 2, 15, 0) as u64, "Monday 07:00 PST is 15:00Z, not 14:00Z");
}

#[test]
fn tz_a_repeating_event_is_nine_oclock_in_july_and_december() {
    let c = cfg_in("America/Los_Angeles");
    let p = plat();
    let mut d = daemon(&c, &p, tmp("cal"));
    let said = "schedule standup every monday at 9";
    let out = d.execute(&Intent::Schedule(said.into()));
    show("tz+calendar", said, &out);
    assert!(out.contains("09:00 PDT") || out.contains("09:00 PST"), "said on your clock: {out}");
    let from = atlas::store::now();
    let occ = d.calendar.occurrences_between(from, from + 120 * 86_400);
    let la = atlas::tz::Zone::named("America/Los_Angeles").unwrap();
    let hours: Vec<u32> = occ.iter().map(|e| la.hour(e.start as i64)).collect();
    let utc_hours: std::collections::BTreeSet<i64> = occ.iter().map(|e| (e.start as i64 % 86_400) / 3600).collect();
    for e in occ.iter().take(3).chain(occ.iter().rev().take(2)) {
        println!("  {} — {}", e.title, e.say_when_in(&la));
    }
    assert!(occ.len() >= 16);
    assert!(hours.iter().all(|h| *h == 9), "every one at 9 local: {hours:?}");
    assert_eq!(utc_hours.into_iter().collect::<Vec<_>>(), vec![16, 17], "16:00Z in summer, 17:00Z in winter");
    // And the file it exports carries the zone, so Outlook keeps it at 9 too.
    let ics = d.calendar.to_ics(from);
    assert!(ics.contains("DTSTART;TZID=America/Los_Angeles:"), "{ics}");
}

#[test]
fn tz_an_outlook_invite_with_its_own_vtimezone_lands_at_the_right_hour() {
    let invite = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Microsoft Corporation//Outlook 16.0//EN\r\n\
BEGIN:VTIMEZONE\r\nTZID:Custom Pacific\r\nBEGIN:STANDARD\r\nDTSTART:16011104T020000\r\nRRULE:FREQ=YEARLY;BYDAY=1SU;BYMONTH=11\r\n\
TZOFFSETFROM:-0700\r\nTZOFFSETTO:-0800\r\nEND:STANDARD\r\nBEGIN:DAYLIGHT\r\nDTSTART:16010311T020000\r\n\
RRULE:FREQ=YEARLY;BYDAY=2SU;BYMONTH=3\r\nTZOFFSETFROM:-0800\r\nTZOFFSETTO:-0700\r\nEND:DAYLIGHT\r\nEND:VTIMEZONE\r\n\
BEGIN:VEVENT\r\nUID:a1\r\nDTSTART;TZID=Custom Pacific:20260929T100000\r\nDTEND;TZID=Custom Pacific:20260929T103000\r\n\
RRULE:FREQ=WEEKLY;BYDAY=TU;COUNT=8\r\nSUMMARY:Weekly sync\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:a2\r\nDTSTART;TZID=Somewhere Unheard Of:20261001T090000\r\nSUMMARY:Mystery\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:a3\r\nDTSTART;TZID=Pacific Standard Time:20261215T090000\r\nSUMMARY:Named by Windows\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let home = atlas::tz::Zone::named("America/New_York").unwrap();
    let mut cal = atlas::calendar::Calendar::default();
    let (n, unknown) = cal.import_ics(invite, 0, &home).unwrap();
    println!("LIVE [tz+vformat+calendar]\n  imported {n}; unknown zones read as home: {unknown:?}");
    assert_eq!(n, 3);
    assert_eq!(unknown, vec!["Somewhere Unheard Of".to_string()]);
    let occ = cal.occurrences_between(utc(2026, 9, 1, 0, 0) as u64, utc(2027, 1, 1, 0, 0) as u64);
    for e in &occ {
        println!("  {} — {} (UTC {})", e.title, e.say_when_in(&home), atlas::digest::iso_utc(e.start));
    }
    let sync: Vec<&atlas::calendar::Event> = occ.iter().filter(|e| e.title == "Weekly sync").collect();
    assert_eq!(sync.len(), 8);
    assert_eq!(sync[0].start, utc(2026, 9, 29, 17, 0) as u64, "10:00 PDT");
    assert_eq!(sync[7].start, utc(2026, 11, 17, 18, 0) as u64, "10:00 PST, after the change");
    let mystery = occ.iter().find(|e| e.title == "Mystery").unwrap();
    assert_eq!(mystery.start, utc(2026, 10, 1, 13, 0) as u64, "09:00 in the home zone (EDT)");
    let win = occ.iter().find(|e| e.title == "Named by Windows").unwrap();
    assert_eq!(win.start, utc(2026, 12, 15, 17, 0) as u64);
}

#[test]
fn tz_unset_is_utc_exactly_as_before() {
    // Pinned here (28 Sep 2026): this is about a time of day, and it passed
    // only when cargo found `.cargo/config.toml` (run from the crate's folder).
    atlas::localclock::pin_offset(Some(0));
    let c = cfg_in("");
    let p = plat();
    let d = daemon(&c, &p, tmp("unset"));
    assert!(d.home_zone().is_utc());
    let s = atlas::settings::registry(&c.tools.clone().unwrap());
    let tz = s.get("time_zone").expect("a hub setting");
    println!("hub setting: {} = {:?}", tz.name, tz.value);
    // Unset reads "Automatic" since the 26 Sep merge: this computer's clock,
    // which the test suite pins to UTC (`ATLAS_CLOCK_OFFSET=0`), hence the
    // `is_utc` above.
    assert!(matches!(&tz.value, atlas::settings::Value::Choice { value, .. } if value == "Automatic"));
    let mut s = s;
    assert!(s.set("time_zone", "Europe/London").is_ok());
    assert!(s.set("time_zone", "Mars/Olympus").is_err(), "only zones it knows are offered");
}

// ---- guessable through the vault ---------------------------------------------

#[test]
fn guessable_the_vault_refuses_a_guessable_first_passphrase_and_keeps_a_good_one() {
    use atlas::vault::{Vault, VaultConfig};
    let cfg = VaultConfig::default();
    let mut v = Vault::default();
    for weak in ["password1234", "aaaaaaaaaaaa", "qwertyuiopas", "atlasatlas2026"] {
        let r = v.open(weak, 0, &cfg);
        println!("LIVE [guessable+vault]\n  first passphrase: {weak}\n  out: {r:?}\n");
        assert!(r.is_err(), "{weak} should be refused");
        assert!(!v.has_a_passphrase(), "a refusal sets nothing");
    }
    v.open("the lamp my sister broke in june", 0, &cfg).expect("a sentence is fine");
    assert!(v.has_a_passphrase());
    // Unlocking later is checked against what was set, never re-judged.
    v.lock();
    assert!(v.open("the lamp my sister broke in june", 0, &cfg).is_ok());
    // Changing to a guessable one is refused, and the old one still works.
    let r = v.change_passphrase("the lamp my sister broke in june", "password1234", 0, &cfg);
    println!("  change to password1234: {r:?}");
    assert!(r.is_err());
    v.lock();
    assert!(v.open("the lamp my sister broke in june", 0, &cfg).is_ok());
}

#[test]
fn tz_doctor_names_the_zone() {
    let c = cfg_in("Pacific Standard Time");
    let found = atlas::doctor::run(&c, c.tools.as_ref(), &plat());
    let line = found.iter().find(|f| f.label == "time zone").expect("a time zone line");
    println!("doctor: time zone — {}", line.detail);
    assert!(line.ok && line.detail.contains("America/Los_Angeles"));
    let c = cfg_in("Nowhere/Special");
    let found = atlas::doctor::run(&c, c.tools.as_ref(), &plat());
    assert!(!found.iter().find(|f| f.label == "time zone").unwrap().ok);
}

// ---- redact through the secondary model ---------------------------------------

#[test]
fn redact_what_reaches_the_online_model_and_what_comes_back() {
    use atlas::brain::{FallbackLlm, Llm};
    use std::sync::{Arc, Mutex};
    struct Down;
    impl Llm for Down {
        fn complete(&self, _: &str, _: &str) -> atlas::error::Result<String> {
            Err(atlas::error::AtlasError::Config("local model offline".into()))
        }
    }
    // Records exactly what it was sent, and answers using the placeholders.
    struct Online(Mutex<Vec<String>>);
    impl Llm for Online {
        fn complete(&self, system: &str, user: &str) -> atlas::error::Result<String> {
            self.0.lock().unwrap().push(format!("{system}\n{user}"));
            let email = user.split_whitespace().find(|w| w.starts_with("⟦EMAIL")).unwrap_or("?").trim_end_matches(|c: char| !c.is_alphanumeric() && c != '⟧');
            Ok(format!("Draft: write to {email} and say the card was blocked."))
        }
    }
    let online = Arc::new(Online(Mutex::new(vec![])));
    let llm = FallbackLlm::new(Arc::new(Down), Some(online.clone()));
    let prompt = "Help me reply to dana.k@acme-install.com about the refund. Card on file 4111 1111 1111 1111, \
                  her SSN 123-45-6789 was on the form, call +1 415 555 0142. My AWS key AKIAIOSFODNN7EXAMPLE is \
                  in the config, and the token ghp_1234567890abcdefghijklmnopqrstuvwxyz12 too. \
                  IBAN GB82 WEST 1234 5698 7654 32. Session eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJlcmljIn0.dBjftJeZ4CVPmB92K27uhbUJU1p1r_wW1gFWFOEjXk. \
                  Order #10492 shipped 2026-09-21 to 1600 Main St. Build hash 3f9a2c1e7b.";
    let reply = llm.complete("You draft emails.", prompt).unwrap();
    let sent = online.0.lock().unwrap()[0].clone();
    println!("LIVE [redact+brain]\n  sent online:\n    {}\n  reply back home:\n    {reply}\n", sent.replace('\n', "\n    "));
    for secret in ["dana.k@acme-install.com", "4111 1111 1111 1111", "123-45-6789", "+1 415 555 0142", "AKIAIOSFODNN7EXAMPLE", "ghp_1234567890", "GB82 WEST", "eyJhbGciOiJIUzI1NiJ9"] {
        assert!(!sent.contains(secret), "{secret} left the machine");
    }
    // Ordinary numbers are not personal numbers.
    for plain in ["#10492", "2026-09-21", "1600 Main St", "3f9a2c1e7b"] {
        assert!(sent.contains(plain), "{plain} should have gone as it was");
    }
    assert_eq!(reply, "Draft: write to dana.k@acme-install.com and say the card was blocked.", "put back at home");

    // Text full of multi-byte characters — em dashes, arrows, accents — is
    // scanned without slicing inside one (it used to panic on "—").
    let mut u = atlas::redact::Scrubber::default();
    let out = u.scrub("Fix it — «as before» → café; key AKIAIOSFODNN7EXAMPLE — ok ⟦not a placeholder⟧ ñ");
    assert_eq!(out, "Fix it — «as before» → café; key ⟦AWS_KEY_1⟧ — ok ⟦not a placeholder⟧ ñ");
    let mut s = atlas::redact::Scrubber::default();
    let a = s.scrub("mail a@b.co then a@b.co again; card 4111-1111-1111-1112 is not a card (Luhn fails)");
    println!("  {a}\n  {}", s.say().unwrap());
    assert_eq!(a.matches("⟦EMAIL_1⟧").count(), 2, "the same original, the same placeholder");
    assert!(a.contains("4111-1111-1111-1112"));
    assert_eq!(s.say().as_deref(), Some("1 email address kept back from the online model"));
    assert!(sent.contains("Use the placeholders exactly as written"), "the model is told");
}

// ---- lookalike + Authentication-Results, as the mail check reads them -------------

#[test]
fn lookalike_senders_that_only_look_like_the_people_you_deal_with() {
    use atlas::lookalike::sender_warning;
    let known = vec!["acme-install.com".to_string(), "northwind.co.uk".to_string(), "homelab.com".to_string()];
    let pass = "mx.google.com; spf=pass smtp.mailfrom=x; dkim=pass header.d=x; dmarc=pass header.from=x";
    let cases: &[(&str, &str, bool)] = &[
        ("Dana <dana@acme-install.com>", pass, false),
        ("Dana <dana@billing.acme-install.com>", pass, false),
        ("Dana <dana@acme-lnstall.com>", pass, true),        // l for i
        ("Dana <dana@acme-instal.com>", pass, true),         // one dropped
        ("Dana <dana@acme-install.co>", pass, true),         // different ending
        ("Dana <dana@acmе-install.com>", pass, true),        // Cyrillic е
        ("Dana <dana@acme-install.com.pay-portal.net>", pass, true),
        ("Accounts <ap@acme-install-invoices.com>", pass, true),
        ("Tom <tom@northwlnd.co.uk>", pass, true),
        ("\"homelab.com support\" <help@mailer-x.io>", pass, true),
        ("Pat <pat@unrelated-bakery.com>", pass, false),
        ("Mum <mum@gmail.com>", pass, false),
        // Right address, but its own server says it's forged.
        ("Dana <dana@acme-install.com>", "mx.google.com; spf=fail smtp.mailfrom=acme-install.com; dkim=none; dmarc=fail (p=REJECT) header.from=acme-install.com", true),
        ("Dana <dana@acme-install.com>", "mx.google.com; spf=softfail; dkim=pass header.d=acme-install.com; dmarc=pass", false),
    ];
    for (from, ar, expect) in cases {
        let w = sender_warning(from, ar, &known);
        println!("LIVE [lookalike]  {from:<50} -> {}", w.as_deref().unwrap_or("fine"));
        assert_eq!(w.is_some(), *expect, "{from} / {ar}");
    }
}

#[test]
fn lookalike_the_imap_fetch_reads_the_topmost_authentication_results() {
    // A scripted IMAP server, driven through the real `uid_fetch`. Only the
    // first header is your provider's; a sender can add their own below it.
    struct Scripted(std::io::Cursor<Vec<u8>>, std::rc::Rc<std::cell::RefCell<Vec<u8>>>);
    impl std::io::Read for Scripted {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
            self.0.read(b)
        }
    }
    impl std::io::Write for Scripted {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.1.borrow_mut().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let header = "From: Dana <dana@acme-install.com>\r\n\
Authentication-Results: mx.google.com;\r\n spf=fail smtp.mailfrom=acme-install.com;\r\n dmarc=fail header.from=acme-install.com\r\n\
Authentication-Results: fake.example; spf=pass; dkim=pass; dmarc=pass\r\n\
Subject: Updated bank details\r\n\r\n";
    let body = "Please use the new account.";
    let script = format!(
        "* 1 FETCH (UID 9 FLAGS () BODY[HEADER.FIELDS (FROM SUBJECT AUTHENTICATION-RESULTS)] {{{}}}\r\n{header} BODY[TEXT] {{{}}}\r\n{body})\r\nA0001 OK FETCH completed\r\n",
        header.len(),
        body.len()
    );
    let sent = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut sess = atlas::imap::Session::new(Scripted(std::io::Cursor::new(script.into_bytes()), sent.clone()));
    let m = sess.uid_fetch(9).unwrap();
    let asked = String::from_utf8_lossy(&sent.borrow()).to_string();
    println!("LIVE [lookalike+imap]\n  asked: {}\n  parsed Authentication-Results: {:?}", asked.trim(), m.authentication_results);
    assert!(asked.contains("AUTHENTICATION-RESULTS"), "the header is asked for");
    assert_eq!(m.from, "Dana <dana@acme-install.com>");
    assert_eq!(m.subject, "Updated bank details", "a header after the two is still read");
    assert_eq!(m.uid, 9);
    assert!(m.authentication_results.starts_with("mx.google.com;") && m.authentication_results.contains("dmarc=fail"));
    let w = atlas::lookalike::sender_warning(&m.from, &m.authentication_results, &["acme-install.com".into()]).unwrap();
    println!("  said: {w}");
    assert!(w.contains("DMARC"));
}

// ---- diff (Myers) ------------------------------------------------------------

#[test]
fn diff_shortest_script_and_the_big_change_count() {
    use atlas::diff::{edits, lines_changed, unified, Edit};
    // The paper's example: ABCABBA -> CBABAC, D = 5.
    let a: Vec<char> = "ABCABBA".chars().collect();
    let b: Vec<char> = "CBABAC".chars().collect();
    let es = edits(&a, &b);
    assert_eq!(es.iter().filter(|e| !matches!(e, Edit::Keep(..))).count(), 5);
    // Replaying the script rebuilds the new sequence.
    let rebuilt: String = es.iter().filter_map(|e| match e {
        Edit::Keep(i, _) => Some(a[*i]),
        Edit::Insert(j) => Some(b[*j]),
        Edit::Delete(_) => None,
    }).collect();
    assert_eq!(rebuilt, "CBABAC");
    assert_eq!(edits::<char>(&[], &[]).len(), 0);
    // Where the old set count went wrong: moving a block of braces.
    let before = "fn a() {\n    x();\n}\n\nfn b() {\n    y();\n}\n";
    let after = "fn b() {\n    y();\n}\n\nfn a() {\n    x();\n}\n";
    let old_count = {
        let (bl, al): (Vec<&str>, Vec<&str>) = (before.lines().collect(), after.lines().collect());
        let common = bl.iter().filter(|l| al.contains(l)).count();
        (bl.len() - common) + (al.len() - common)
    };
    println!("LIVE [diff+selfwork]\n  swapped two functions: set count {old_count}, Myers {}", lines_changed(before, after));
    assert_eq!(old_count, 0, "the old count saw nothing");
    assert_eq!(lines_changed(before, after), 8);
    let edit = |c: &str| atlas::selfwork::Edit { path: "src/x.rs".into(), content: c.into(), reason: String::new() };
    assert_eq!(atlas::selfwork::lines_touched(&[edit(before)], &[edit(after)]), 8, "what selfwork now reports");
    let u = unified("a\nb\nc\nd\ne\nf\ng\n", "a\nb\nC\nd\ne\nf\ng\nh\n", "old", "new", 1);
    println!("{u}");
    assert!(u.contains("@@ -2,3 +2,3 @@\n b\n-c\n+C\n d\n"), "{u}");
    assert!(u.contains("+h\n"));
}

// ---- yata (shared text) through the sync log --------------------------------------

#[test]
fn yata_two_devices_edit_apart_and_come_back_to_the_same_page() {
    use atlas::yata::{from_log, Doc, Op};
    // Into a device's log the way the running Atlas does it: queued, then taken.
    let record = |log: &mut atlas::sync::Log, doc: &str, ops: &[Op], at: u64| {
        let inbox = tmp(&format!("rec-{}-{at}", log.device));
        atlas::yata::queue(&inbox, doc, ops).unwrap();
        atlas::yata::take_queued(&inbox, log, at);
    };
    // Randomised: three sites, concurrent edits, delivered in shuffled orders.
    let mut seed = 7u64;
    let mut rnd = |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % n.max(1) as u64) as usize
    };
    for round in 0..40 {
        let mut docs = vec![Doc::new("laptop"), Doc::new("phone"), Doc::new("tablet")];
        let base = docs[0].insert(0, "shared notes");
        for d in docs.iter_mut().skip(1) {
            for o in &base {
                d.apply(o.clone());
            }
        }
        let mut all: Vec<Op> = base.clone();
        for (k, d) in docs.iter_mut().enumerate() {
            for _ in 0..3 {
                let len = d.text().chars().count();
                let ops = if rnd(3) == 0 && len > 0 {
                    let at = rnd(len);
                    d.delete(at, 1 + rnd(2))
                } else {
                    let at = rnd(len + 1);
                    d.insert(at, ["A", "bb", "ccc", " x"][(k + rnd(4)) % 4])
                };
                all.extend(ops);
            }
        }
        // Everyone receives everything, each in a different order.
        let mut texts = Vec::new();
        for site in ["laptop", "phone", "tablet", "fresh"] {
            let mut d = Doc::new(site);
            let mut ops = all.clone();
            for i in (1..ops.len()).rev() {
                ops.swap(i, rnd(i + 1));
            }
            for o in ops {
                d.apply(o);
            }
            assert_eq!(d.waiting(), 0);
            texts.push(d.text());
        }
        assert!(texts.windows(2).all(|w| w[0] == w[1]), "round {round}: {texts:?}");
    }

    // Through the real sync log: two devices, apart, then a merge.
    let mut laptop = atlas::sync::Log::new("laptop");
    let mut d = Doc::new("laptop");
    let ops = d.set_text("Installer: call Tuesday.\nBudget: 4k.\n");
    record(&mut laptop, "house", &ops, 100);
    let mut phone = atlas::sync::Log::new("phone");
    phone.events = laptop.events.clone();
    // Apart: the laptop rewrites a line, the phone adds one.
    let mut dl = from_log("house", "laptop", &laptop.events);
    let ops = dl.set_text("Installer: call Wednesday.\nBudget: 4k.\n");
    record(&mut laptop, "house", &ops, 200);
    let mut dp = from_log("house", "phone", &phone.events);
    let ops = dp.set_text("Installer: call Tuesday.\nBudget: 4k.\nPermit: ask the council.\n");
    record(&mut phone, "house", &ops, 210);
    let merged = atlas::sync::merge(&laptop.events, &phone.events, 300);
    assert!(merged.clashes.is_empty(), "text edits never clash");
    let mut both = laptop.events.clone();
    for e in &phone.events {
        if !both.iter().any(|x| x.device == e.device && x.seq == e.seq) {
            both.push(e.clone());
        }
    }
    let text = from_log("house", "laptop", &both).text();
    println!("LIVE [yata+sync]\n  laptop: Tuesday -> Wednesday; phone: added the permit line\n  merged:\n{text}");
    assert_eq!(text, "Installer: call Wednesday.\nBudget: 4k.\nPermit: ask the council.\n");
    assert_eq!(atlas::yata::all_names(&both, std::path::Path::new("/nonexistent")), vec!["house".to_string()]);
}

// ---- drain (log templates) ----------------------------------------------------------

#[test]
fn drain_a_log_of_thousands_of_lines_reads_as_a_dozen_things() {
    let mut log = String::new();
    let mut t = 1_790_000_000u64;
    for i in 0..600 {
        t += 37;
        log.push_str(&format!("{t} INFO checked mail in {} ms, {} new\n", 400 + i % 900, i % 4));
        log.push_str(&format!("{t} INFO selected: {} button\n", ["Send", "Reply", "Archive"][i % 3]));
        if i % 50 == 0 {
            log.push_str(&format!("{t} WARN research fetch of https://site{}.example/page failed: timed out after 30 s\n", i));
        }
        if i % 120 == 7 {
            log.push_str(&format!("{t} WARN the vault is sealed; skipped mail account {}\n", ["work", "home"][i % 2]));
        }
    }
    let (d, lines) = atlas::drain::read_log(&log);
    println!("LIVE [drain]  {lines} lines -> {} templates", d.templates.len());
    for tpl in d.by_count() {
        println!("  {:>5}  {}", tpl.count, tpl.text());
    }
    assert_eq!(lines, 1200 + 12 + 5);
    assert!(d.templates.len() <= 6, "{}", d.templates.len());
    let top = d.by_count();
    assert_eq!(top[0].count + top[1].count, 1200);
    assert!(top.iter().any(|t| t.text() == "WARN research fetch of <*> failed: timed out after <*> s" && t.count == 12));
    assert!(top.iter().any(|t| t.text().starts_with("WARN the vault is sealed; skipped mail account") && t.count == 5));
}

// ---- bandit (Thompson sampling) in what Atlas says first ------------------------------

#[test]
fn bandit_a_nudge_and_an_offer_are_weighed_by_how_you_answered_them() {
    use atlas::awareness::Signals;
    use atlas::memory::Memory;
    use atlas::proactive::{Offer, Proactive, ProactiveConfig};
    let signals = Signals {
        active: Some(atlas::platform::ActiveWindow { process: "chrome.exe".into(), title: "docs".into() }),
        dwell_secs: 60,
        idle_secs: 120,
        recent_changes: atlas::index::Changes { added: (0..6).map(|i| format!("/f{i}")).collect(), ..Default::default() },
        in_conversation: false,
        ..Default::default()
    };
    let nudge = Offer { kind: "nudge_checkin".into(), message: "How's the proposal going?".into(), command: String::new(), confidence: 0.6, cost: 0 };
    let mut m = Memory::default();
    for _ in 0..8 {
        m.record_approval("offer:nudge_checkin", true, None);
    }
    m.record_approval("offer:index_new_files", false, None);
    let (mut to_nudge, mut to_offer) = (0, 0);
    for tick in 0..300u64 {
        let mut pr = Proactive::new(ProactiveConfig { enabled: true, cooldown_secs: 60, ..Default::default() });
        match pr.consider_with(&signals, &m, 10_000 + tick * 61, Some(nudge.clone())).map(|o| o.kind) {
            Some(k) if k == "nudge_checkin" => to_nudge += 1,
            Some(_) => to_offer += 1,
            None => {}
        }
    }
    println!("LIVE [bandit+proactive]  welcomed 8/8 vs 0/1: chose the nudge {to_nudge}x, the file offer {to_offer}x of 300");
    assert_eq!(to_nudge + to_offer, 300);
    assert!(to_nudge > 200, "the welcomed kind is favoured");
    // P(Beta(1,2) > Beta(9,1)) = 9·B(9,3) ≈ 1.8%, so ~5 of 300 expected.
    assert!((2..=15).contains(&to_offer), "the other is still tried — it used to be never: {to_offer}");
    // Once asked, the cooldown holds for the next tick whichever was chosen.
    let mut pr = Proactive::new(ProactiveConfig { enabled: true, cooldown_secs: 60, ..Default::default() });
    assert!(pr.consider_with(&signals, &m, 5000, Some(nudge.clone())).is_some());
    assert!(pr.consider_with(&signals, &m, 5030, Some(nudge)).is_none(), "a nudge now counts against the interruption budget");
}

// ---- vad + diarize ---------------------------------------------------------------------

/// A synthetic room: steady fan noise, and "voices" — harmonic stacks at a
/// speaker's own pitch with syllable-rate loudness changes.
fn room(voices: &[(f64, f64, f64)], total_s: f64, fan_db: f64, seed: u64) -> Vec<i16> {
    let rate = 16_000.0;
    let n = (total_s * rate) as usize;
    let mut x = seed;
    let fan = 10f64.powf(fan_db / 20.0);
    (0..n)
        .map(|i| {
            let t = i as f64 / rate;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let noise = ((x % 20001) as f64 / 10000.0 - 1.0) * fan * 1.7;
            let mut v = 0.0;
            for (f0, from, to) in voices {
                if t >= *from && t < *to {
                    let syll = 0.55 + 0.45 * (2.0 * std::f64::consts::PI * 4.0 * t).sin().abs();
                    for h in 1..=8 {
                        v += (0.25 / h as f64) * (2.0 * std::f64::consts::PI * f0 * h as f64 * t).sin() * syll;
                    }
                }
            }
            ((v * 0.5 + noise).clamp(-1.0, 1.0) * 32767.0) as i16
        })
        .collect()
}

#[test]
fn vad_a_fan_above_the_fixed_line_no_longer_holds_the_turn_open() {
    use atlas::endpoint::{EndpointConfig, Endpointer, Listening};
    let cfg = EndpointConfig::default();
    // 0.5 s of fan, 1.5 s of "speech", then fan only. The fan is at -30 dBFS:
    // above the fixed -38 line, so on loudness alone the turn never ends.
    let audio = room(&[(140.0, 0.5, 2.0)], 8.0, -30.0, 3);
    let run = |use_vad: bool| -> (Listening, u64) {
        let mut ep = Endpointer::start(0);
        let mut vad = atlas::vad::Vad::new(16_000);
        for (w, win) in audio.chunks(4000).enumerate() {
            let now = (w as u64 + 1) * 250;
            let db = atlas::audio::level_db(win);
            let share = vad.window(win);
            let level = if use_vad { atlas::vad::level_for_endpoint(db, share, cfg.silence_below_db) } else { db };
            if use_vad && w < 12 {
                println!("    {now} ms: {db:.1} dB, speech share {share:?} -> {level:.1}");
            }
            let st = ep.feed(level, "", now, &cfg);
            if ep.finished() {
                return (st, now);
            }
        }
        (Listening::Speaking, 8000)
    };
    let (old, old_ms) = run(false);
    let (new, new_ms) = run(true);
    println!("LIVE [vad+endpoint]  fan at -30 dBFS, speech 0.5–2.0 s:\n  loudness only: {old:?} at {old_ms} ms\n  with the detector: {new:?} at {new_ms} ms");
    assert!(!matches!(old, Listening::Finished(atlas::endpoint::Why::YouFinished)), "the fixed line never hears the end");
    assert!(matches!(new, Listening::Finished(atlas::endpoint::Why::YouFinished)), "{new:?}");
    assert!((2000..=3500).contains(&new_ms), "ends soon after the speech: {new_ms}");
    // And the segments of that recording are where the speech was.
    let segs = atlas::vad::segments(&audio, 16_000);
    println!("  segments: {:?}", segs.iter().map(|(s, e)| (*s as f64 / 16000.0, *e as f64 / 16000.0)).collect::<Vec<_>>());
    assert_eq!(segs.len(), 1);
    assert!((segs[0].0 as f64 / 16000.0 - 0.5).abs() < 0.15 && (segs[0].1 as f64 / 16000.0 - 2.0).abs() < 0.3);
}

#[test]
fn diarize_three_turns_two_voices_and_you() {
    // Two speakers alternating; a stand-in encoder that measures pitch, the
    // way a real one separates voices (it is the external tool's job).
    let audio = room(&[(120.0, 0.5, 2.0), (230.0, 2.8, 4.0), (120.0, 4.8, 6.0), (230.0, 6.8, 7.6)], 8.5, -45.0, 9);
    let pitch_of = |s: &[i16]| -> f64 {
        // Autocorrelation peak between 70 and 400 Hz.
        let x: Vec<f64> = s.iter().take(4000).map(|v| *v as f64).collect();
        let (mut best, mut lag) = (f64::MIN, 0);
        for l in 40..230 {
            let c: f64 = x.iter().zip(x.iter().skip(l)).map(|(a, b)| a * b).sum();
            if c > best {
                best = c;
                lag = l;
            }
        }
        16000.0 / lag as f64
    };
    let mut embed = |s: &[i16]| -> Option<Vec<f32>> {
        let p = pitch_of(s);
        Some((0..16).map(|k| ((k as f64 + 1.0) * p / 400.0).sin() as f32).collect())
    };
    let mut n = 0;
    let mut hear = |_: &[i16]| -> Option<String> {
        n += 1;
        Some(format!("turn {n}"))
    };
    let mine: Vec<f32> = embed(&audio[8000..32000]).unwrap(); // enrolled on the 120 Hz voice
    let lines = atlas::diarize::who_said_what(&audio, 16_000, &mut embed, &mut hear, Some(&mine), 0.72);
    println!("LIVE [vad+diarize]");
    for l in &lines {
        println!("  {}", l.say());
    }
    let who: Vec<&str> = lines.iter().map(|l| l.speaker.as_str()).collect();
    assert_eq!(who, vec!["You", "Speaker 2", "You", "Speaker 2"]);
    // A WAV round trip through the reader the CLI uses.
    let wav = atlas::audio::wav_bytes(&audio, 16_000);
    let (back, rate) = atlas::diarize::read_wav(&wav).unwrap();
    assert_eq!((back.len(), rate), (audio.len(), 16_000));
}

// ---- agefile (age v1, X25519) — checked against the real `age` tool --------------------

#[test]
fn agefile_the_real_age_tool_opens_what_atlas_seals_and_the_other_way() {
    let dir = tmp("age");
    if std::process::Command::new("age").arg("--version").output().is_err() {
        println!("SKIP: `age` is not installed here, so the cross-check can't run.");
        return;
    }
    // Keys from age-keygen, read by Atlas.
    let kg = std::process::Command::new("age-keygen").output().unwrap();
    let keyfile = String::from_utf8(kg.stdout).unwrap();
    let their_secret = keyfile.lines().find(|l| l.starts_with("AGE-SECRET-KEY-")).unwrap().to_string();
    let their_public = keyfile.lines().find_map(|l| l.strip_prefix("# public key: ")).unwrap().to_string();
    assert_eq!(atlas::agefile::recipient_of(&their_secret).unwrap(), their_public, "same public key from the same secret");
    std::fs::write(dir.join("key.txt"), &keyfile).unwrap();

    // Atlas seals (two chunks and a bit), `age -d` opens.
    let plain: Vec<u8> = (0..150_000u32).map(|i| (i * 7 % 251) as u8).collect();
    let (my_secret, my_public) = atlas::agefile::new_identity();
    let sealed = atlas::agefile::seal(&plain, &[their_public.clone(), my_public.clone()]).unwrap();
    std::fs::write(dir.join("a.age"), &sealed).unwrap();
    let out = std::process::Command::new("age").args(["-d", "-i"]).arg(dir.join("key.txt")).arg(dir.join("a.age")).output().unwrap();
    println!("LIVE [agefile]\n  Atlas sealed 150,000 bytes to an age-keygen key and its own; `age -d`: {}", if out.status.success() { "opened" } else { "FAILED" });
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(out.stdout, plain);
    assert_eq!(atlas::agefile::open(&sealed, &my_secret).unwrap(), plain, "and Atlas opens its own copy");

    // `age -r` seals, Atlas opens.
    std::fs::write(dir.join("b.txt"), b"contract v3, signed pages 1-4").unwrap();
    let st = std::process::Command::new("age").args(["-r", &my_public, "-o"]).arg(dir.join("b.age")).arg(dir.join("b.txt")).status().unwrap();
    assert!(st.success());
    let from_age = std::fs::read(dir.join("b.age")).unwrap();
    let opened = atlas::agefile::open(&from_age, &my_secret).unwrap();
    println!("  `age -r` sealed to Atlas's key; Atlas opened: {:?}", String::from_utf8_lossy(&opened));
    assert_eq!(opened, b"contract v3, signed pages 1-4");

    // Wrong key, changed header, changed body: each refused, by name.
    let (other, _) = atlas::agefile::new_identity();
    assert_eq!(atlas::agefile::open(&from_age, &other).unwrap_err(), "it isn't sealed to this key");
    let mut bent = from_age.clone();
    let i = bent.windows(3).position(|w| w == b"---").unwrap() + 5;
    bent[i] = if bent[i] == b'A' { b'B' } else { b'A' };
    assert!(atlas::agefile::open(&bent, &my_secret).is_err());
    let mut bent = from_age.clone();
    let last = bent.len() - 3;
    bent[last] ^= 1;
    assert_eq!(atlas::agefile::open(&bent, &my_secret).unwrap_err(), "the contents were changed or cut short");
    assert!(atlas::agefile::seal(b"x", &["age1notakey".into()]).is_err());
}

#[test]
fn yata_edits_from_the_command_line_reach_the_log_through_the_running_atlas() {
    // `atlas doc` never writes the sync log (the daemon is its one writer and
    // would overwrite it); it queues, and the next tick takes the queue in.
    let c = cfg_in("");
    let p = plat();
    let root = tmp("docinbox");
    let inbox = Store::new(root.clone()).data_dir().join("doc-inbox");
    let mut d = daemon(&c, &p, root.clone());
    let site = "this device".to_string(); // the name a fresh log gets with sync.name unset
    let none: Vec<atlas::sync::Event> = Vec::new();
    let mut page = atlas::yata::current("house", &site, &none, &inbox);
    atlas::yata::queue(&inbox, "house", &page.set_text("Installer: call Tuesday.\n")).unwrap();
    let mut page = atlas::yata::current("house", &site, &none, &inbox);
    assert_eq!(page.text(), "Installer: call Tuesday.\n", "a queued edit is part of the page already");
    atlas::yata::queue(&inbox, "house", &page.set_text("Installer: call Tuesday.\nBudget: 4k.\n")).unwrap();
    assert_eq!(std::fs::read_dir(&inbox).unwrap().count(), 2);
    d.tick(atlas::store::now());
    let saved: atlas::sync::Log = Store::new(root.clone()).load("synclog");
    println!("LIVE [yata+daemon]  2 queued edits -> {} events in the saved sync log; inbox now {} files", saved.events.len(), std::fs::read_dir(&inbox).unwrap().count());
    assert!(!saved.events.is_empty());
    assert_eq!(std::fs::read_dir(&inbox).unwrap().count(), 0, "taken once the log was saved");
    assert_eq!(atlas::yata::from_log("house", &site, &saved.events).text(), "Installer: call Tuesday.\nBudget: 4k.\n");
    assert_eq!(atlas::yata::all_names(&saved.events, &inbox), vec!["house".to_string()]);
}

// ---- pronounce (GAPS.md §B) -----------------------------------------------------------

#[test]
fn pronounce_the_words_the_speech_engine_mangles() {
    let mut mine = std::collections::BTreeMap::new();
    mine.insert("Jo-2".to_string(), "jo two".to_string());
    let cases = [
        ("Maya's EURUSD note is +1.5R, ~3k words.", "Maya's euro dollar note is plus one point five R, about three thousand words."),
        ("The VPS → the NAS & back: 98% done — ASAP.", "The V P S to the N A S and back: ninety-eight percent done, ASAP."),
        ("Ask Jo-2 about GBP/JPY, see https://x.example/y", "Ask jo two about pound yen, see a link"),
        ("Two APIs and the PIN.", "Two A P Is and the PIN."),
    ];
    for (written, said) in cases {
        let got = atlas::pronounce::for_speech(written, &mine);
        println!("LIVE [pronounce]  {written}\n                -> {got}");
        assert_eq!(got, said);
    }
    // The shipped setting reads, and is empty by default.
    let c = cfg_in("");
    assert!(c.tools.as_ref().unwrap().pronounce.is_empty());
}

// ---- the sync folder, checked on the configured cadence (dead_config: check_every_hours) ----

#[test]
fn cloudsync_the_folder_is_checked_for_still_syncing() {
    use atlas::cloudsync::{still_syncing, CloudConfig, Trouble};
    let dir = tmp("syncfolder");
    let cfg = CloudConfig::default(); // every 12 hours
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    std::fs::write(dir.join("laptop.bundle"), "{}").unwrap();
    // Alone in the folder: nothing to expect.
    assert!(still_syncing(&dir, "laptop.bundle", false, now, &cfg).is_none());
    // Another device was seen once, but nothing of theirs is here.
    let r = still_syncing(&dir, "laptop.bundle", true, now, &cfg).unwrap();
    assert_eq!(r.0, Trouble::NotActuallySyncing);
    // Their bundle arrives: fine.
    std::fs::write(dir.join("phone.bundle"), "{}").unwrap();
    assert!(still_syncing(&dir, "laptop.bundle", true, now, &cfg).is_none());
    // Two days later with nothing new from them.
    let r = still_syncing(&dir, "laptop.bundle", true, now + 48 * 3600, &cfg).unwrap();
    println!("LIVE [cloudsync]  48 h later: {} -> {}", r.1, r.0.fix());
    assert_eq!(r.0, Trouble::NotActuallySyncing);
    assert!(r.1.contains("48 hours"));
    // The folder gone.
    let r = still_syncing(&dir.join("nope"), "laptop.bundle", true, now, &cfg).unwrap();
    assert_eq!(r.0, Trouble::Moved);
}

#[test]
fn panel_the_waking_panel_leaves_by_itself() {
    let c = cfg_in("");
    let p = plat();
    let mut d = daemon(&c, &p, tmp("waking"));
    let t0 = atlas::store::now();
    d.execute(&Intent::Ready);
    assert_eq!(d.wants_panel, Some(atlas::panel::Panel::Waking));
    d.tick(t0 + 3);
    assert_eq!(d.wants_panel, Some(atlas::panel::Panel::Waking), "still up after 3 s");
    d.tick(t0 + 10);
    println!("LIVE [panel]  Waking panel shown at t0; t0+3: up; t0+10: {:?} (waking_secs 9)", d.wants_panel);
    assert_eq!(d.wants_panel, None, "gone after waking_secs");
    // Only the transient one fades; a panel you asked for stays however long.
    let pc = atlas::panel::PanelConfig::default();
    assert!(!atlas::panel::faded(atlas::panel::Panel::Tasks, 0, 1_000_000, &pc));
    assert!(atlas::panel::faded(atlas::panel::Panel::Waking, 0, pc.waking_secs, &pc));
}

// ---- a full disk: no silent backups (GAPS.md §C) ----------------------------------------

#[test]
fn full_disk_a_fix_that_cannot_keep_its_backup_does_not_land() {
    let dir = tmp("nobackup");
    let target = dir.join("config.txt");
    std::fs::write(&target, "original").unwrap();
    let staged = dir.join("staged.txt");
    std::fs::write(&staged, "changed").unwrap();
    // `keep` can't take files: it's a file, not a folder — the same failure
    // a full disk gives, without filling one.
    let keep = dir.join("keep-is-a-file");
    std::fs::write(&keep, "x").unwrap();
    let change = atlas::sandbox::Change { target: target.clone(), source: staged, new_file: false, bytes: 7, target_was: None };
    let r = atlas::selfwork::land(&[change], &keep);
    println!("LIVE [selfwork]  backup folder unwritable -> {r:?}; the file still reads {:?}", std::fs::read_to_string(&target).unwrap());
    assert!(r.is_err());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "original", "nothing overwritten without its backup");
}

// ---- zipread: looking inside archives (dead_config: look_inside_archives) --------------------

#[test]
fn zipread_find_the_note_inside_a_zip_and_refuse_a_bomb() {
    use atlas::index::{Index, IndexConfig};
    let dir = tmp("zips");
    let work = tmp("zips-work");
    std::fs::write(work.join("budget.md"), "# Q3\n\nThe installer budget is four thousand, with the permit on top.\n\n# Q4\n\nNothing yet.\n").unwrap();
    std::fs::write(work.join("notes.txt"), "stored, not compressed: the heat pump quote came in\n").unwrap();
    let big: String = "all work and no play ".repeat(20_000);
    std::fs::write(work.join("long.txt"), &big).unwrap();
    // The real zip tool, deflate for two, stored (-0) for one.
    let z = |args: &[&str]| std::process::Command::new("zip").current_dir(&work).args(args).output().map(|o| o.status.success()).unwrap_or(false);
    if !z(&["-q", dir.join("Q3-handover.zip").to_str().unwrap(), "budget.md", "long.txt"]) {
        println!("SKIP: no zip tool here");
        return;
    }
    assert!(z(&["-q", "-0", dir.join("Q3-handover.zip").to_str().unwrap(), "notes.txt"]));
    let idx_cfg = IndexConfig { roots: vec![dir.to_string_lossy().to_string()], exclude_dirs: vec![], exclude_exts: vec![], max_depth: 8, max_enrich_mb: 20 };
    let index = Index::scan(&idx_cfg);
    for q in ["installer budget permit", "heat pump quote", "all work and no play"] {
        let hits = index.search_content(q, &idx_cfg, 3);
        println!("LIVE [zipread+index]  \"{q}\" -> {:?}", hits.iter().map(|h| h.cite.clone()).collect::<Vec<_>>());
        assert!(!hits.is_empty(), "{q}");
    }
    let hits = index.search_content("installer budget permit", &idx_cfg, 3);
    assert_eq!(hits[0].cite, "Q3-handover.zip › budget.md:1-3", "the file inside, and the lines");
    assert!(hits[0].excerpt.contains("four thousand"));
    // A zip whose claimed size is past the guard's limit is not inflated.
    let bytes = std::fs::read(dir.join("Q3-handover.zip")).unwrap();
    let tight = atlas::files::FilesConfig { max_unpacked_mb: 0, ..Default::default() };
    let refused = atlas::zipread::texts_inside(&bytes, &tight, 1 << 30);
    println!("  with a 0 MB unpack limit: {refused:?}");
    assert!(refused.is_err());
    // Every member comes out byte for byte (CRC checked) against the real files.
    let got = atlas::zipread::texts_inside(&bytes, &atlas::files::FilesConfig::default(), 1 << 30).unwrap();
    assert_eq!(got.iter().find(|(n, _)| n == "long.txt").unwrap().1, big);
    assert_eq!(got.len(), 3);
}

/// Authentication failure still warns, even with a signature; passing headers
/// are not authorization for an automatic client reply.
#[test]
fn sender_checks_preserve_explicit_failure_even_with_a_signature() {
    let known = vec!["client.com".to_string()];
    let from = "Dana <dana@client.com>";
    assert!(atlas::lookalike::sender_warning(from, "mx; dkim=pass; dmarc=fail", &known).unwrap().contains("DMARC"));
    assert!(atlas::lookalike::sender_warning(from, "mx; spf=fail; dkim=fail", &known).unwrap().contains("SPF"));
    assert!(atlas::lookalike::sender_warning(from, "mx; dkim=pass; dmarc=pass", &known).is_none());
    assert!(atlas::lookalike::sender_warning("Dana <dana@c1ient.com>", "mx; dkim=pass", &known).is_some());
}
