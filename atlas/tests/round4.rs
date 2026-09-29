//! Round 4: closing the gaps the earlier rounds named. Each test drives the
//! fix through the part of Atlas that uses it, with what went in and what
//! came out printed (`cargo test --test all round4 -- --nocapture`).

// ---- readable on real pages (round-1 gap G4) ------------------------------------------

#[test]
fn readable_on_three_real_rendered_pages() {
    // Rendered by Chromium from the live sites on 23 Sep 2026 (live/crawl/),
    // after checking each domain is one this environment may fetch.
    let cases: &[(&str, &[&str], &[&str])] = &[
        (
            "mdn-content-disposition",
            &["indicates whether content should be displayed inline", "filename* is preferred over filename when both are understood"],
            &["Skip to main content", "Website Privacy Notice", "Learn web development"],
        ),
        (
            "go-loopvar",
            &["removing one of the most common Go mistakes", "The Problem"],
            &["Why Go", "Copyright"],
        ),
        (
            "python-classes",
            &["Classes provide a means of bundling data and functionality together", "valedictorian = max((student.gpa, student.name) for student in graduates)"],
            &["Previous topic", "Report a bug", "Show source", "Python Software Foundation"],
        ),
    ];
    for (f, keep, drop) in cases {
        let html = std::fs::read_to_string(format!("tests/fixtures/crawl/{f}.html")).unwrap();
        let t = atlas::research::page_text(&html);
        let old = atlas::research::strip_html(&html);
        println!("LIVE [readable] {f}: kept {} of {} characters\n  starts: {}\n", t.len(), old.len(), t.chars().take(160).collect::<String>().replace('\n', " / "));
        // The furniture went and the article stayed: less text than the
        // tag-stripper keeps, and the first thing worth keeping is near the top.
        assert!(t.len() < old.len(), "{f}: {} kept of {}", t.len(), old.len());
        let first = t.find(keep[0]).unwrap_or(usize::MAX);
        assert!(first < 2_000, "{f}: the article starts {first} characters in");
        for k in *keep {
            assert!(t.contains(k), "{f}: lost {k:?}");
        }
        for d in *drop {
            assert!(old.contains(d), "{f}: the page itself has {d:?} (so its absence below means something)");
            assert!(!t.contains(d), "{f}: kept the furniture {d:?}");
        }
    }
}

// ---- shared helpers ----------------------------------------------------------------------

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-r4-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cfg_in(zone: &str) -> Config {
    let mut c = Config::load(Path::new("config")).unwrap();
    c.tools.as_mut().unwrap().time_zone = zone.into();
    c
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, root: PathBuf) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(root), Proactive::new(ProactiveConfig::default()))
}

fn b64(bytes: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            s.push(if i <= c.len() { A[((n >> (18 - 6 * i)) & 63) as usize] as char } else { '=' });
        }
    }
    s
}

// ---- hub: import and export by clicking (round-1 gap G2) ---------------------------------

#[test]
fn hub_bring_in_an_invite_and_contacts_and_download_them_back() {
    use atlas::server::Action;
    let c = cfg_in("America/Los_Angeles");
    let p = plat();
    let root = tmp("hub-io");
    let mut d = daemon(&c, &p, root.clone());
    let ics = std::fs::read("../live/outlook_invite.ics").unwrap_or_else(|_| {
        b"BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:x1\r\nDTSTART;TZID=Pacific Standard Time:20261006T100000\r\nSUMMARY:Ops sync\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n".to_vec()
    });
    let r = atlas::hublive::reply(&mut d, Action::BringIn { name: "invite.ics".into(), base64: b64(&ics) });
    println!("LIVE [hub bring-in .ics]  {}", r.body);
    assert!(r.body.contains("added or updated from invite.ics"), "{}", r.body);
    let vcf = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Dana Kim\r\nEMAIL:dana@acme-install.com\r\nEND:VCARD\r\nBEGIN:VCARD\r\nVERSION:3.0\r\nFN:Dana Kim\r\nEMAIL:dana.kim@acme-install.com\r\nEND:VCARD\r\n";
    let r = atlas::hublive::reply(&mut d, Action::BringIn { name: "contacts.vcf".into(), base64: b64(vcf.as_bytes()) });
    println!("LIVE [hub bring-in .vcf]  {}", r.body);
    assert!(r.body.contains("added to your clients from contacts.vcf"), "{}", r.body);
    let cal = atlas::hublive::reply(&mut d, Action::ExportCalendar);
    let head = atlas::server::render(&cal);
    println!("LIVE [hub download]  {}", head.lines().take(4).collect::<Vec<_>>().join(" | "));
    assert!(head.contains("Content-Disposition: attachment; filename=\"calendar.ics\""));
    assert!(head.contains("text/calendar"));
    assert!(cal.body.contains("BEGIN:VEVENT"));
    let vc = atlas::hublive::reply(&mut d, Action::ExportClients);
    assert!(vc.body.contains("dana@acme-install.com"));
    // And the buttons are on the Calendar page.
    let page = atlas::hub::calendar_page(&atlas::calendar::Calendar::default(), 0, &atlas::tz::Zone::utc());
    assert!(page.contains("href=\"/hub/calendar.ics\"") && page.contains("id=bring-in-file"));
    // The routes reach those actions.
    let req = |m: &str, path: &str, body: &str| {
        atlas::server::route(&atlas::server::Request {
            method: m.into(),
            path: path.into(),
            query: String::new(),
            token: Some("x".repeat(24)),
            token_from_url: false,
            body: body.into(),
        })
    };
    assert_eq!(req("GET", "/hub/calendar.ics", ""), Some(Action::ExportCalendar));
    assert_eq!(req("POST", "/hub/bring-in", "{\"name\":\"a.ics\",\"data\":\"QQ==\"}"), Some(Action::BringIn { name: "a.ics".into(), base64: "QQ==".into() }));
}

// ---- the nudger is told when its nudge was not said (round-3 gap) -----------------------

#[test]
fn nudge_a_greeting_that_lost_to_an_offer_is_still_owed() {
    use atlas::nudge::{NudgeConfig, Nudger, Trigger};
    let mut n = Nudger::new(NudgeConfig::default());
    let t = 1_790_150_400u64; // a weekday morning
    let first = n.consider(t, 8, 0, 0).expect("the morning greeting");
    assert_eq!(first.trigger, Trigger::Daypart);
    assert!(n.consider(t + 60, 8, 0, 0).is_none(), "once said, not again this morning");
    // This time it lost to an offer on the same tick.
    let mut n = Nudger::new(NudgeConfig::default());
    let lost = n.consider(t, 8, 0, 0).unwrap();
    n.unsaid(&lost);
    let again = n.consider(t + 60, 8, 0, 0);
    println!("LIVE [nudge]  greeting lost to an offer -> next tick: {:?}", again.as_ref().map(|x| x.message.clone()));
    assert_eq!(again.map(|x| x.trigger), Some(Trigger::Daypart), "still owed, because it was never heard");
}

// ---- booking: your hours on your clock (round-3 gap) --------------------------------------

#[test]
fn booking_nine_to_six_means_where_you_are() {
    use atlas::booking::{assess, could_offer, BookingConfig, Fit, Proposal, Slot};
    let la = atlas::tz::Zone::named("America/Los_Angeles").unwrap();
    let cfg = BookingConfig::default(); // 9 to 18
    let now = 1_790_150_400u64; // Tue 22 Sep 2026? any fixed instant
    let day = now + 3 * 86_400;
    let at_local = |h: i64| la.to_utc(la.to_local(day as i64).div_euclid(86_400) * 86_400 + h * 3600) as u64;
    let p = Proposal {
        id: 1,
        from: "dana@acme-install.com".into(),
        about: None,
        their_words: "how about 10 or 7am".into(),
        times: vec![Slot { start: at_local(10), mins: 30 }, Slot { start: at_local(7), mins: 30 }],
        at: now,
        state: atlas::booking::State::NeedsYou,
    };
    let a = assess(&p, &[], now, &cfg, &la);
    println!("LIVE [booking]  10:00 PDT -> {:?}; 07:00 PDT -> {:?}", a[0].verdict, a[1].verdict);
    assert_eq!(a[0].verdict, Fit::Free, "10:00 in Los Angeles is inside 9–18 there");
    assert_eq!(a[1].verdict, Fit::OffHours);
    let utc = assess(&p, &[], now, &cfg, &atlas::tz::Zone::utc());
    println!("  on the old UTC rule: 07:00 PDT (14:00 UTC) -> {:?}", utc[1].verdict);
    assert_eq!(utc[1].verdict, Fit::Free, "the old rule offered 7 a.m. as inside your hours");
    for s in could_offer(&[], now, 30, &cfg, 5, &la) {
        let h = la.hour(s.start as i64);
        assert!((9..18).contains(&h), "offered {h}:00 local");
    }
}

// ---- fit: pinning a tier, and re-planning ------------------------------------------------

#[test]
fn fit_force_tier_pins_the_plan_both_ways() {
    use atlas::fit::{plan_for, plan_as_set, FitConfig, Machine, Tier};
    let small = Machine { total_ram_mb: 8192, free_ram_mb: 3000, cpu_cores: 4, disk_free_mb: 50_000, ..Default::default() };
    let measured = plan_for(&small);
    let up = plan_as_set(&small, &FitConfig { force_tier: "full".into(), ..Default::default() });
    let down = plan_as_set(&small, &FitConfig { force_tier: "voice".into(), ..Default::default() });
    println!("LIVE [fit]  measured {:?} ({:?}); pinned full -> {:?}; pinned voice -> model {:?}\n  {}", measured.tier, measured.model, up.model, down.model, up.because);
    assert_eq!(up.tier, Tier::Full);
    assert!(up.model.unwrap().contains("7b"));
    assert!(up.because.contains("Pinned"));
    assert_eq!(down.tier, Tier::Voice);
    assert!(down.model.is_none() && down.speech.is_some());
    assert_eq!(plan_as_set(&small, &FitConfig::default()), measured, "unset measures");
    let c = cfg_in("");
    assert!(c.tools.as_ref().unwrap().fit.replan_on_change, "the shipped setting now reaches a type");
}

// ---- helpers that die are started again ---------------------------------------------------

#[test]
#[cfg(unix)] // runs `sh` / sets Unix file modes
fn lifecycle_a_helper_that_dies_comes_off_the_books() {
    use atlas::lifecycle::{Helpers, LifecycleConfig};
    let mut h = Helpers::new(LifecycleConfig::default());
    let spawn = || std::process::Command::new("sh").args(["-c", "exit 3"]).spawn().map(Some).map_err(|e| e.to_string());
    h.want("model-server", 100, 0, spawn).unwrap();
    h.done("model-server", 0);
    assert!(h.is_running("model-server"));
    std::thread::sleep(std::time::Duration::from_millis(300));
    let gone = h.died();
    println!("LIVE [lifecycle]  died: {gone:?}");
    assert_eq!(gone, vec![("model-server".to_string(), "exit code 3".to_string())]);
    assert!(!h.is_running("model-server"), "off the books, so the next want starts it");
    let mut started = false;
    h.want("model-server", 100, 1, || {
        started = true;
        Ok(None)
    })
    .unwrap();
    assert!(started, "started again rather than reused");
}

// ---- contact duplicates: u measured on the list (round-1 gap G3) --------------------------

#[test]
fn linkage_chance_agreement_is_measured_on_the_list() {
    use atlas::linkage::{Contact, Model};
    let first = ["Ana", "Ben", "Cara", "Dev", "Eli", "Fay", "Gus", "Hana", "Ivo", "Jun"];
    let last = ["Smith", "Garcia", "Chen", "Patel", "Okafor", "Novak", "Kim"];
    let mut list = Vec::new();
    for (i, f) in first.iter().enumerate() {
        for (j, l) in last.iter().enumerate() {
            list.push(Contact { name: format!("{f} {l}"), email: format!("{}.{}@{}.com", f.to_lowercase(), l.to_lowercase(), ["acme", "north", "kiwi"][(i + j) % 3]), phone: String::new() });
        }
    }
    let (m, note) = Model::measured_on(&list);
    let d = Model::default();
    println!("LIVE [linkage]  {} contacts: {note}\n  u(same name) chosen {:.4} -> measured {:.4}; u(names close) {:.4} -> {:.4}", list.len(), d.name_exact.u, m.name_exact.u, d.name_close.u, m.name_close.u);
    assert!(note.contains("measured on your list"));
    assert!(m.name_exact.u < 0.001, "no two people here share a name");
    assert!((m.name_else.u + m.name_near.u + m.name_close.u + m.name_exact.u - 1.0).abs() < 1e-9);
    let (_, few) = Model::measured_on(&list[..10]);
    assert!(few.contains("too few"));
}

// ---- the updates folder: noticed, checked, swapped on start ------------------------------

#[test]
#[cfg(unix)] // runs `sh` / sets Unix file modes
fn upgrade_a_dropped_binary_is_checked_then_swapped_in_with_the_old_one_kept() {
    use std::os::unix::fs::PermissionsExt;
    let root = tmp("updates");
    let running = root.join("atlas");
    std::fs::write(&running, "#!/bin/sh\necho old\n").unwrap();
    assert!(atlas::upgrade::waiting(&root).is_none(), "nothing dropped, nothing waiting");
    std::fs::create_dir_all(root.join("updates")).unwrap();
    let new = root.join("updates/atlas");
    // Something that isn't Atlas: refused, and said why.
    std::fs::write(&new, "#!/bin/sh\necho hello\n").unwrap();
    std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755)).unwrap();
    let r = atlas::upgrade::waiting(&root).unwrap();
    println!("LIVE [updates]  a stray file: {r:?}");
    assert!(r.is_err());
    // A real-looking new version.
    std::fs::write(&new, "#!/bin/sh\ncase \"$1\" in --health-check) echo 'healthy: atlas 9.9.9';; *) echo 'atlas 9.9.9';; esac\n").unwrap();
    std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755)).unwrap();
    let (_, v) = atlas::upgrade::waiting(&root).unwrap().unwrap();
    assert_eq!(v, "9.9.9");
    // Named by tag (version + fingerprint) since 28 Sep 2026.
    let old_tag = atlas::upgrade::tag_of(&running, atlas::upgrade::version());
    let new_tag = atlas::upgrade::tag_of(&new, "9.9.9");
    let swapped = atlas::upgrade::swap_checked(&root, &running, std::time::Duration::from_secs(20)).unwrap().unwrap();
    println!("LIVE [updates]  swapped in {v}; old kept as {}", atlas::upgrade::keep_old_at(&root, &old_tag).display());
    assert_eq!(swapped, atlas::upgrade::Swapped::Started { path: running.clone(), version: "9.9.9".into(), tag: new_tag, previous: old_tag.clone() });
    assert!(std::fs::read_to_string(&running).unwrap().contains("9.9.9"));
    assert_eq!(std::fs::read_to_string(atlas::upgrade::keep_old_at(&root, &old_tag)).unwrap(), "#!/bin/sh\necho old\n", "the old one kept for going back");
    assert!(!new.exists());
}

// ---- `atlas doc` with no Atlas running writes the log itself, under the lock --------------

#[test]
fn doc_without_a_running_atlas_reaches_the_log() {
    let home = tmp("doc-cli");
    let run = |args: &[&str]| {
        let o = std::process::Command::new(env!("CARGO_BIN_EXE_atlas"))
            .args(args)
            .env("ATLAS_HOME", &home)
            .env("ATLAS_CONFIG", std::fs::canonicalize("config").unwrap())
            .output()
            .unwrap();
        String::from_utf8_lossy(&o.stdout).to_string()
    };
    println!("LIVE [doc, no daemon]  {}", run(&["doc", "set", "house", "Installer: call Tuesday."]).trim());
    let store = Store::new(home.join("data").join("state"));
    let inbox = store.data_dir().join("doc-inbox");
    let left = std::fs::read_dir(&inbox).map(|d| d.count()).unwrap_or(0);
    let log: atlas::sync::Log = store.load("synclog");
    println!("  inbox files left: {left}; events in the sync log: {}", log.events.len());
    assert_eq!(left, 0, "taken in, because nothing else was running");
    assert_eq!(atlas::yata::from_log("house", &log.device, &log.events).text(), "Installer: call Tuesday.");
}

// ---- diarize on real (synthesized) speech: two voices, four turns ---------------------------

#[test]
fn diarize_real_speech_segments_land_on_the_right_speaker() {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string("tests/fixtures/speech/truth.json").unwrap()).unwrap();
    let turns: Vec<(usize, usize, String)> = v["turns"].as_array().unwrap().iter()
        .map(|x| (x[0].as_u64().unwrap() as usize, x[1].as_u64().unwrap() as usize, x[2].as_str().unwrap().to_string())).collect();
    // A stand-in encoder that knows only pitch (autocorrelation), as in
    // round 3: the real encoder is an external tool not installed here.
    let pitch_of = |s: &[i16]| -> f64 {
        let x: Vec<f64> = s.iter().take(8000).map(|v| *v as f64).collect();
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
    for room in ["quiet", "fan10"] {
        let (audio, rate) = atlas::diarize::read_wav(&std::fs::read(format!("tests/fixtures/speech/call_{room}.wav")).unwrap()).unwrap();
        // A histogram of pitch over the segment's half-second windows, on a
        // log scale: one reading per segment swings 200–296 Hz with
        // intonation (measured on voice B), and a single-reading stand-in
        // split B into two speakers. Real encoders summarise a segment the
        // same way; the one Atlas runs is an external tool.
        let mut embed = |s: &[i16]| -> Option<Vec<f32>> {
            let mut h = vec![0f32; 16];
            for w in s.chunks(8000).filter(|w| w.len() >= 4000) {
                let p = pitch_of(w).clamp(70.0, 400.0);
                let bin = ((p / 70.0).ln() / (400f64 / 70.0).ln() * 15.0).round() as usize;
                for (k, v) in h.iter_mut().enumerate() {
                    *v += (-((k as f32 - bin as f32).powi(2)) / 2.0).exp();
                }
            }
            (h.iter().sum::<f32>() > 0.0).then_some(h)
        };
        let mut hear = |_: &[i16]| -> Option<String> { None };
        let a_turn = &turns[0];
        let mine = embed(&audio[a_turn.0..a_turn.1]).unwrap();
        let lines = atlas::diarize::who_said_what(&audio, rate, &mut embed, &mut hear, Some(&mine), 0.72);
        let (mut right, mut total) = (0, 0);
        for l in &lines {
            let mid = ((l.start_ms + l.end_ms) / 2 * rate as u64 / 1000) as usize;
            let Some(t) = turns.iter().find(|t| t.0 <= mid && mid < t.1) else { continue };
            total += 1;
            let want = if t.2 == "A" { "You" } else { "Speaker 2" };
            right += (l.speaker == want) as usize;
        }
        println!("LIVE [diarize, real speech, {room}]  {} segments; {right}/{total} on the right speaker", lines.len());
        for l in &lines {
            println!("    {}", l.say());
        }
        assert!(total >= 4, "every turn found");
        assert_eq!(right, total, "{room}");
    }
}

#[test]
fn clarifying_questions_switch_on_the_settings_page_now_reaches_something() {
    // The toggle was drawn as `true` whatever the file said; turning it off
    // did nothing. Now it reads `wanted.ask_when_unclear`.
    let said = "The certification is going to take six weeks.";
    let p = plat();
    let on = cfg_in("");
    let mut d = daemon(&on, &p, tmp("wanted-on"));
    let asked = d.turn(said, 1_790_000_000);
    println!("LIVE [clarifying questions on]  {asked}");
    assert_eq!(asked, atlas::wanted::ask_which(), "on: it asks");

    let mut off = cfg_in("");
    off.tools.as_mut().unwrap().wanted.ask_when_unclear = false;
    let mut d = daemon(&off, &p, tmp("wanted-off"));
    let reply = d.turn(said, 1_790_000_000);
    println!("LIVE [clarifying questions off] {reply}");
    assert_ne!(reply, atlas::wanted::ask_which(), "off: it goes on to answer instead");
    let page = atlas::settings::registry(off.tools.as_ref().unwrap());
    let item = page.items.iter().find(|i| i.key == "wanted.ask_when_unclear").unwrap();
    println!("  settings page shows: {:?}", item.value);
    assert!(matches!(item.value, atlas::settings::Value::Toggle(false)), "the page shows the file");
}
