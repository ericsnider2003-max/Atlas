//! Round 9: a working day, not just a conversation — where the time went,
//! offers held for a natural break, a cue when you come back, and times
//! said the way people say them (`cargo test --test all round9 -- --nocapture`).

#[path = "when_corpus.rs"]
mod when_corpus;

use atlas::civil::days_from_civil;

const DAY: u64 = 86_400;

fn corpus_now() -> u64 {
    days_from_civil(2026, 9, 23) as u64 * DAY + 10 * 3600 + 15 * 60
}

fn expected_secs(e: (i64, u32, u32, u32, u32)) -> (u64, bool) {
    let d = days_from_civil(e.0, e.1, e.2) as u64 * DAY;
    if e.3 == 99 {
        (d, true)
    } else {
        (d + (e.3 * 3600 + e.4 * 60) as u64, false)
    }
}

/// Score a reader over the phrase book: right, wrong (a sure answer that
/// isn't the person's), and declined (not sure, or nothing) where a person
/// would have given an answer.
fn score(set: &[(&str, Option<(i64, u32, u32, u32, u32)>)], read: &dyn Fn(&str, u64) -> Option<(u64, bool, bool)>) -> (usize, usize, usize, Vec<String>) {
    let (mut right, mut wrong, mut declined) = (0, 0, 0);
    let mut misses = Vec::new();
    for (phrase, want) in set {
        let got = read(phrase, corpus_now());
        match (want, got) {
            (None, None) | (None, Some((_, _, false))) => right += 1,
            (None, Some((s, _, true))) => {
                wrong += 1;
                misses.push(format!("{phrase:?}: should have asked, said {s}"));
            }
            (Some(e), Some((s, all_day, true))) => {
                if expected_secs(*e) == (s, all_day) {
                    right += 1;
                } else {
                    wrong += 1;
                    misses.push(format!("{phrase:?}: wanted {:?}, got {} (all day {all_day})", e, { let c = atlas::civil::Civil::from_local(s as i64); format!("{}-{:02}-{:02} {:02}:{:02}", c.year, c.month, c.day, c.hour, c.minute) }));
                }
            }
            (Some(_), _) => {
                declined += 1;
                misses.push(format!("{phrase:?}: declined"));
            }
        }
    }
    (right, wrong, declined, misses)
}

#[test]
fn times_are_read_the_way_people_say_them() {
    let n = when_corpus::CORPUS.len();
    let new = score(when_corpus::CORPUS, &|p, now| atlas::when::parse(p, now).map(|x| (x.start, x.all_day, x.sure)));
    println!("LIVE [when] {n} phrases: {} right, {} wrong, {} declined", new.0, new.1, new.2);
    for m in &new.3 {
        println!("    {m}");
    }
    let cal = score(when_corpus::CORPUS, &|p, now| atlas::calendar::resolve_when(p, now).map(|w| (w.start, w.all_day, true)));
    println!("LIVE [when] the calendar's reader over the same {n}: {} right, {} wrong, {} declined", cal.0, cal.1, cal.2);
    let h = when_corpus::HELD_OUT.len();
    let held = score(when_corpus::HELD_OUT, &|p, now| atlas::when::parse(p, now).map(|x| (x.start, x.all_day, x.sure)));
    println!("LIVE [when] held out ({h} phrases, written after tuning): {} right, {} wrong, {} declined", held.0, held.1, held.2);
    for m in &held.3 {
        println!("    {m}");
    }
    // First run of the held-out set, before any fix: 38 right, 1 wrong ("a
    // week from today" read as today), 1 declined (11/5 — by design: both
    // readings are dates). The one wrong was a general bug (the word "today"
    // read again after "from"), fixed; the decline stays.
    assert_eq!(held.1, 0, "{:?}", held.3);
    assert_eq!(new.1, 0, "a sure answer that isn't the person's is the one failure a calendar can't have");
    assert!(new.0 * 100 >= n * 95, "{} of {n}", new.0);
}

// ===================== where the time went (worklog) =====================

use atlas::platform::mock::MockPlatform;
use atlas::platform::{ActiveWindow, Monitor, OsQuiet};
use atlas::worklog::{summarise, Beat, WorkLog, WorkLogConfig};

fn win(app: &str, title: &str) -> ActiveWindow {
    ActiveWindow { process: app.into(), title: title.into() }
}

/// A morning, as heartbeats every 5 s: (from, to, window or away).
fn morning() -> Vec<(u64, u64, Option<ActiveWindow>)> {
    let h = |x: f64| (x * 3600.0) as u64;
    vec![
        (h(9.0), h(9.0) + 20 * 60, Some(win("Code.exe", "worklog.rs - atlas"))),
        (h(9.0) + 20 * 60, h(9.0) + 20 * 60 + 40, Some(win("slack.exe", "general"))), // a 40 s glance
        (h(9.0) + 20 * 60 + 40, h(9.0) + 50 * 60, Some(win("Code.exe", "when.rs - atlas"))),
        (h(9.0) + 50 * 60, h(10.0) + 5 * 60, Some(win("OUTLOOK.EXE", "Inbox - Outlook"))),
        (h(10.0) + 5 * 60, h(10.0) + 15 * 60, None), // away ten minutes
        (h(10.0) + 15 * 60, h(11.0), Some(win("terminal64.exe", "EURUSD,M15 - MetaTrader"))),
        (h(11.0), h(11.0) + 3 * 60, Some(win("chrome.exe", "news - Google Chrome"))), // 3 min: breaks focus
        (h(11.0) + 3 * 60, h(11.0) + 30 * 60, Some(win("terminal64.exe", "EURUSD,M15 - MetaTrader"))),
    ]
}

fn play(log: &mut WorkLog, cfg: &WorkLogConfig, day: &[(u64, u64, Option<ActiveWindow>)]) {
    let mut last_input = 0u64;
    let mut last_window: Option<ActiveWindow> = None;
    for (from, to, w) in day {
        let mut t = *from;
        while t < *to {
            match w {
                Some(w) => {
                    last_input = t;
                    last_window = Some(w.clone());
                    log.beat(cfg, t, Some(w), Some(0));
                }
                None => {
                    log.beat(cfg, t, last_window.as_ref(), Some(t - last_input));
                }
            }
            t += 5;
        }
    }
}

#[test]
fn a_morning_is_kept_as_where_the_time_went() {
    let cfg = WorkLogConfig::default();
    let mut log = WorkLog::default();
    play(&mut log, &cfg, &morning());
    let s = summarise(&log.spans);
    let clock = |t: u64| format!("{}:{:02}", (t % 86_400) / 3600, (t % 3600) / 60);
    let said = atlas::worklog::say(&s, "this morning", false, &clock);
    println!("LIVE [worklog] {} spans kept from {} heartbeats. {said}", log.spans.len(), (11.5 * 3600.0 - 9.0 * 3600.0) as u64 / 5);
    let cat = |c: &str| s.by_category.iter().find(|(x, _)| x == c).map(|(_, n)| *n).unwrap_or(0);
    // Every second accounted for: 2 h 30 at the desk minus the 10-min break,
    // give or take one heartbeat at each edge.
    assert!((s.active as i64 - (150 - 10) * 60).abs() <= 10, "{}", s.active);
    assert!((cat("coding") as i64 - (50 * 60 - 40)).abs() <= 10, "{}", cat("coding"));
    assert!((cat("mail") as i64 - 15 * 60).abs() <= 10);
    assert!((cat("trading") as i64 - (45 + 27) * 60).abs() <= 10, "{}", cat("trading"));
    // Focus: the coding stretch survives a 40 s glance at Slack; the trading
    // stretch before the 3-minute news break is 45 min, after it 27 min.
    let blocks: Vec<(String, u64)> = s.blocks.iter().map(|b| (b.category.clone(), (b.end - b.start + 30) / 60)).collect();
    assert_eq!(blocks, vec![("coding".to_string(), 50), ("trading".to_string(), 45), ("trading".to_string(), 27)], "{blocks:?}");
    assert_eq!(s.switches, 6, "coding→chat→coding→mail→trading→browsing→trading");
    // Away was seen and ended the mail span at the last input.
    let mail = log.spans.iter().find(|x| x.category == "mail").unwrap();
    assert_eq!(mail.end, 10 * 3600 + 5 * 60 - 5);
}

#[test]
fn coming_back_names_what_you_were_in_the_middle_of() {
    let cfg = WorkLogConfig::default();
    let mut log = WorkLog::default();
    play(&mut log, &cfg, &morning()[..4]);
    let left = 10 * 3600 + 5 * 60;
    let c = log.last_context(left).unwrap();
    println!("LIVE [worklog] back at 10:15: {}", c.cue());
    assert_eq!(c.category, "mail");
    assert!(c.cue().contains("Inbox - Outlook"));
    // Without keyboard timing, away can't be told from working, and the
    // record says so rather than pretending.
    let mut blind = WorkLog::default();
    assert_eq!(blind.beat(&cfg, 100, Some(&win("Code.exe", "x")), None), Beat::Active);
    assert!(!blind.saw_input && blind.blind_beats == 1);
}

// ===================== offers held for a natural break ====================

use atlas::awareness::Awareness;
use atlas::index::Index;
use atlas::proactive::{Proactive, ProactiveConfig};

/// A working day at one-second resolution: typing in stretches in one app,
/// a switch every 20–45 minutes, short pauses, and two breaks. Deterministic.
struct Day {
    /// (second, app, input idle seconds)
    at: Vec<(u64, &'static str, u64)>,
}

fn working_day() -> Day {
    let apps = ["Code.exe", "terminal64.exe", "OUTLOOK.EXE", "Code.exe", "chrome.exe", "Code.exe", "terminal64.exe", "WINWORD.EXE"];
    let mut x = 0x1234_5678_9abc_def0u64;
    let mut rnd = |n: u64| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x % n
    };
    let mut at = Vec::new();
    let mut t = 0u64;
    let mut last_input = 0u64;
    for (k, app) in apps.iter().enumerate() {
        let stint = (20 + rnd(26)) * 60;
        let end = t + stint;
        while t < end {
            // Type for 20–90 s, then pause 2–15 s (a pause of 20 s+ now and then).
            let burst = 20 + rnd(71);
            let pause = if rnd(10) == 0 { 25 + rnd(20) } else { 2 + rnd(14) };
            for _ in 0..burst {
                if t >= end {
                    break;
                }
                last_input = t;
                at.push((t, *app, 0));
                t += 1;
            }
            for _ in 0..pause {
                if t >= end {
                    break;
                }
                at.push((t, *app, t - last_input));
                t += 1;
            }
        }
        if k == 3 || k == 6 {
            // A break away from the desk.
            for _ in 0..(12 * 60) {
                at.push((t, *app, t - last_input));
                t += 1;
            }
        }
    }
    Day { at }
}

/// Offers arriving through the day, one every 11 minutes, queued; whatever
/// is waiting is said when Atlas may speak (several at one break is what
/// bounded deferral does). Returns (said, said while typing, mean wait, longest
/// wait, said during the quiet window).
fn run(day: &Day, defer: bool, quiet: Option<(u64, u64)>) -> (usize, usize, u64, u64, usize) {
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut aw = Awareness::default();
    let mut index = Index::default();
    let cfg = ProactiveConfig { enabled: true, cooldown_secs: 0, max_interruptions_per_hour: 0, min_idle_secs: 0, defer_while_working: defer, ..Default::default() };
    let mut pro = Proactive::new(cfg);
    let (mut delivered, mut mid_typing, mut total_wait, mut worst, mut during_quiet) = (0, 0, 0u64, 0u64, 0);
    let mut waiting: Vec<u64> = Vec::new();
    for &(t, app, idle) in &day.at {
        *p.active.borrow_mut() = Some(win(app, "work"));
        *p.input_idle.borrow_mut() = Some(idle);
        let in_quiet = quiet.map(|(a, b)| t >= a && t < b).unwrap_or(false);
        *p.quiet.borrow_mut() = Some(if in_quiet { OsQuiet::Presenting } else { OsQuiet::Accepts });
        if t % 660 == 0 && t > 0 {
            waiting.push(t);
        }
        let s = aw.observe(&p, &mut index, None, false, t);
        if !waiting.is_empty() && pro.may_interrupt(&s, t) {
            for since in waiting.drain(..) {
                delivered += 1;
                if idle < 3 {
                    mid_typing += 1;
                }
                if in_quiet {
                    during_quiet += 1;
                }
                total_wait += t - since;
                worst = worst.max(t - since);
            }
        }
    }
    (delivered, mid_typing, total_wait / delivered.max(1) as u64, worst, during_quiet)
}

#[test]
fn offers_wait_for_a_natural_break_but_never_past_the_bound() {
    let day = working_day();
    let hours = day.at.len() as f64 / 3600.0;
    let (n0, typing0, wait0, _, _) = run(&day, false, None);
    let (n1, typing1, wait1, worst1, _) = run(&day, true, None);
    println!(
        "LIVE [deferral] a {hours:.1}-hour synthetic day, an offer every 11 min: before, {n0} said, {typing0} while you were typing \
         (mean wait {wait0} s); held for a break, {n1} said, {typing1} while typing, mean wait {wait1} s, longest {worst1} s"
    );
    assert!(typing1 * 4 <= typing0, "held offers should mostly miss the typing: {typing1} vs {typing0}");
    assert!(worst1 <= ProactiveConfig::default().defer_max_secs, "never past the bound: {worst1}");
    assert!(n1 + 1 >= n0, "holding delays offers, it doesn't drop them: {n1} vs {n0}");
    assert!(worst_bound_ok(worst1));
    // Presenting for an hour: nothing lands on the presentation, whatever the bound.
    let (_, _, _, worst_p, on_screen) = run(&day, true, Some((3600, 7200)));
    println!("LIVE [deferral] presenting from 1:00 to 2:00: {on_screen} offers landed on it; the longest wait {worst_p} s");
    assert_eq!(on_screen, 0);
    // The presentation holds past the bound — the one place that's right.
    assert!(worst_p > ProactiveConfig::default().defer_max_secs, "{worst_p}");
}

fn worst_bound_ok(w: u64) -> bool {
    w <= ProactiveConfig::default().defer_max_secs
}

// ===================== presence from the keyboard =========================

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::store::Store;

fn tmp(tag: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-r9-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn typing_without_speaking_is_not_being_away_and_a_real_break_gets_a_cue() {
    let c = Config::load(std::path::Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("presence")), Proactive::new(ProactiveConfig::default()));
    let t0 = atlas::store::now();
    *p.active.borrow_mut() = Some(win("Code.exe", "worklog.rs - atlas"));
    // An hour and a half of typing, not a word to Atlas.
    let mut t = t0;
    while t < t0 + 5400 {
        *p.input_idle.borrow_mut() = Some(2);
        d.tick(t);
        t += 30;
    }
    let said = d.turn("what's outstanding", t);
    println!("LIVE [presence] after 90 min of silent typing: {said:?}");
    assert!(!said.contains("While you were away") && !said.contains("Before the break"), "{said}");
    // Then a real break: 40 minutes with the keyboard untouched.
    let left = t;
    while t < left + 2400 {
        *p.input_idle.borrow_mut() = Some(t - left);
        d.tick(t);
        t += 30;
    }
    *p.input_idle.borrow_mut() = Some(0);
    d.tick(t);
    let said = d.turn("what's outstanding", t + 5);
    println!("LIVE [presence] back after 40 min away: {said:?}");
    assert!(said.contains("Before the break you'd been in coding"), "{said}");
    // And the time is on record: the ninety minutes at the keyboard, and
    // not the forty away — asked the way you'd ask.
    let kept: u64 = d.worklog.spans.iter().map(|s| s.secs()).sum();
    assert!((kept as i64 - 5400).abs() <= 60, "{kept}");
    let spent = d.turn("where did my time go today", t + 10);
    println!("LIVE [presence] you: \"where did my time go today\"  Atlas: {spent}");
    assert!(spent.contains("coding"), "{spent}");
}

#[test]
fn categories_come_from_your_rules_first_then_the_built_in_ones() {
    use atlas::worklog::{category_for, duration_words, Rule};
    let mine = vec![Rule { category: "homelab".into(), matches: vec!["EURUSD".into()] }];
    assert_eq!(category_for("terminal64.exe", "EURUSD,M15", &mine), "homelab");
    assert_eq!(category_for("terminal64.exe", "EURUSD,M15", &[]), "trading");
    assert_eq!(category_for("KeePassXC.exe", "passwords", &[]), "keepassxc");
    // "password" holds "word", and must not read as writing.
    assert_ne!(category_for("chrome.exe", "Change your password", &[]), "writing");
    assert_eq!(duration_words(0), "under a minute");
    assert_eq!(duration_words(45 * 60), "45 min");
    assert_eq!(duration_words(3 * 3600 + 20 * 60), "3 h 20 min");
    assert_eq!(duration_words(2 * 3600), "2 h");
}

#[test]
fn the_machine_itself_says_how_long_since_you_touched_it() {
    // On Windows this asks the real OS (GetLastInputInfo and
    // SHQueryUserNotificationState); elsewhere the platform says it can't.
    let p = atlas::platform::here();
    let idle = p.input_idle_secs();
    let quiet = p.quiet_state();
    println!("LIVE [this machine] input idle: {idle:?}; the OS on interrupting: {quiet:?}");
    if cfg!(windows) {
        let idle = idle.expect("Windows says how long since the last input");
        assert!(idle < 86_400 * 50, "{idle}");
        assert!(quiet.is_some());
    } else {
        assert_eq!(idle, None);
        assert_eq!(quiet, None);
    }
}

#[test]
fn a_laptop_shut_for_lunch_is_a_break_too() {
    // No tick runs while the machine sleeps, so the keyboard's silence is
    // never seen; the gap in the ticks is what says you were gone.
    let c = Config::load(std::path::Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("lid")), Proactive::new(ProactiveConfig::default()));
    let t0 = atlas::store::now();
    *p.active.borrow_mut() = Some(win("terminal64.exe", "EURUSD,M15 - MetaTrader"));
    let mut t = t0;
    while t < t0 + 2400 {
        *p.input_idle.borrow_mut() = Some(1);
        d.tick(t);
        t += 20;
    }
    // Lid shut for an hour; opened, a key pressed.
    t += 3600;
    *p.input_idle.borrow_mut() = Some(3);
    d.tick(t);
    let said = d.turn("what's outstanding", t + 5);
    println!("LIVE [presence] after an hour with the lid shut: {said:?}");
    let spans = d.worklog.spans.len();
    assert!(spans >= 1 && d.worklog.spans[0].secs() >= 2300, "{:?}", d.worklog.spans);
    assert!(said.contains("Before the break you'd been in trading"), "{said}");
}
