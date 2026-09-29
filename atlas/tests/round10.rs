//! Round 10: the gaps a look over round 9 found, closed — times that were
//! read sure-but-wrong, reminders dropped in silence, a booking that didn't
//! say when, meetings counted as time away, a stalled answer read as a
//! break, a bound on held offers that never fired, and a crash on a name
//! with a dotted capital I (`cargo test --test all round10 -- --nocapture`).

use atlas::civil::{days_from_civil, Civil};
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::{ActiveWindow, Monitor};
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;

const DAY: u64 = 86_400;

/// Wednesday 23 September 2026, 10:15, local.
fn wed() -> u64 {
    days_from_civil(2026, 9, 23) as u64 * DAY + 10 * 3600 + 15 * 60
}

fn at(y: i64, m: u32, d: u32, h: u32, min: u32) -> u64 {
    days_from_civil(y, m, d) as u64 * DAY + (h * 3600 + min * 60) as u64
}

fn show(s: u64) -> String {
    let c = Civil::from_local(s as i64);
    format!("{}-{:02}-{:02} {:02}:{:02}", c.year, c.month, c.day, c.hour, c.minute)
}

fn tmp(tag: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-r10-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn daemon<'a>(p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    // Leaked: a daemon borrows its config for its whole life, and a test's
    // daemon lives to the end of the test.
    let c: &'static Config = Box::leak(Box::new(daytime(Config::load(std::path::Path::new("config")).unwrap())));
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

fn screen() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn win(app: &str, title: &str) -> ActiveWindow {
    ActiveWindow { process: app.into(), title: title.into() }
}

// ============================ times in words ==============================

/// Each of these came back from `when` sure and wrong before this round.
#[test]
fn times_that_were_read_sure_but_wrong() {
    let now = wed();
    // (phrase, now, expected start, expected length in minutes)
    let cases: Vec<(&str, u64, Option<(u64, Option<u64>)>)> = vec![
        ("review tomorrow 11-1pm", now, Some((at(2026, 9, 24, 11, 0), Some(120)))),
        ("tomorrow 10-12pm", now, Some((at(2026, 9, 24, 10, 0), Some(120)))),
        ("tomorrow from 11 to 1pm", now, Some((at(2026, 9, 24, 11, 0), Some(120)))),
        ("tomorrow 2pm-4pm", now, Some((at(2026, 9, 24, 14, 0), Some(120)))),
        ("friday 2:00pm-3:30pm", now, Some((at(2026, 9, 25, 14, 0), Some(90)))),
        ("tomorrow 9am\u{2013}10am", now, Some((at(2026, 9, 24, 9, 0), Some(60)))),
        ("in 3 days at 5", now, Some((at(2026, 9, 26, 17, 0), None))),
        ("in 2 days to look at the report", now, Some((at(2026, 9, 25, 0, 0), None))),
        ("tonight at 1", at(2026, 9, 23, 21, 0), Some((at(2026, 9, 24, 1, 0), None))),
        ("tonight at 12", at(2026, 9, 23, 21, 0), Some((at(2026, 9, 24, 0, 0), None))),
        ("tomorrow at quarter to one", now, Some((at(2026, 9, 24, 12, 45), None))),
        ("quarter to one tonight", at(2026, 9, 23, 21, 0), Some((at(2026, 9, 24, 0, 45), None))),
        ("in 1 month", at(2027, 1, 31, 9, 0), Some((at(2027, 2, 28, 0, 0), None))),
        ("in 2 months", at(2027, 1, 31, 9, 0), Some((at(2027, 3, 31, 0, 0), None))),
        // Asked about, not guessed.
        ("today at 9am", now, None),
        ("tomorrow at 10", at(2026, 9, 24, 0, 30), None),
    ];
    let mut wrong = Vec::new();
    for (phrase, now, want) in &cases {
        let got = atlas::when::parse(phrase, *now);
        let ok = match (want, &got) {
            (None, None) => true,
            (None, Some(p)) => !p.sure,
            (Some((s, mins)), Some(p)) => p.sure && p.start == *s && (mins.is_none() || p.mins == *mins),
            (Some(_), None) => false,
        };
        println!(
            "LIVE [when] {phrase:?} -> {}",
            got.as_ref().map(|p| format!("{} sure={} mins={:?} why={:?}", show(p.start), p.sure, p.mins, p.why)).unwrap_or("nothing".into())
        );
        if !ok {
            wrong.push(format!("{phrase:?}: {got:?}"));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn the_length_of_a_meeting_is_not_its_reminder() {
    use atlas::calendar::reminder_from;
    assert_eq!(reminder_from("meeting tomorrow at 3 for 2 hours, remind me an hour before"), Some(60));
    assert_eq!(reminder_from("call at 4 for 30 minutes, remind me 5 minutes before"), Some(5));
    assert_eq!(reminder_from("review at 2 for half an hour, remind me"), Some(10));
    assert_eq!(reminder_from("standup at 9 for 15 minutes"), None);
    assert_eq!(reminder_from("with a 30-minute reminder"), Some(30));
}

// ============================ the calendar ================================

#[test]
fn a_booking_says_back_the_time_it_booked() {
    let p = screen();
    let mut d = daemon(&p, "booking");
    let t = atlas::store::now();
    // Something already on the calendar sooner than the new booking: before
    // this round the reply looked up "the next event", found this one, and
    // said no time at all for the one just booked.
    let first = d.turn("schedule standup tomorrow at 9am", t);
    let said = d.turn("schedule dentist in 3 days at 4pm", t + 5);
    println!("LIVE [booking] {first:?}\nLIVE [booking] {said:?}");
    assert!(said.contains("16:00") && said.contains("\"dentist\""), "{said}");
}

#[test]
fn a_booking_is_titled_without_its_time() {
    use atlas::calendar::event_title;
    for (asked, title) in [
        ("schedule dentist in 3 days at 4pm", "dentist"),
        ("book review from 2 to 4pm", "review"),
        ("launch party oct 3 at 7pm", "launch party"),
        ("3d print session tomorrow", "3d print session"),
        ("lunch with sam tomorrow", "lunch with sam"),
        ("planning 10/15 at 9", "planning"),
        ("call the bank tmrw at 10", "call the bank"),
        ("team sync the last friday of every month at 4pm", "team sync"),
    ] {
        assert_eq!(event_title(asked), title, "{asked}");
    }
}

// ============================ reminders ===================================

#[test]
fn a_reminder_says_when_or_asks_and_is_never_dropped_in_silence() {
    let p = screen();
    let mut d = daemon(&p, "remind");
    let t = atlas::store::now();
    let said = d.turn("remind me in 3 days at 5 to call mom", t);
    println!("LIVE [remind] {said:?}");
    assert!(said.contains("17:00") && said.contains("call mom"), "{said}");
    let said = d.turn("remind me in 3 days to call the bank", t + 5);
    println!("LIVE [remind] {said:?}");
    assert!(said.contains("what time"), "{said}");
    let said = d.turn("remind me on 3/4 at noon to pay rent", t + 10);
    println!("LIVE [remind] {said:?}");
    assert!(said.contains("haven't set it"), "{said}");
    let said = d.turn("set a reminder for tomorrow at 9 to water the plants", t + 15);
    println!("LIVE [remind] {said:?}");
    assert!(said.contains("09:00") && said.contains("water the plants"), "{said}");
}

// ============================ what reaches "improve" ======================

#[test]
fn everyday_sentences_are_not_project_work() {
    use atlas::intent::{Intent, Parser};
    let c = Config::load(std::path::Path::new("config")).unwrap();
    let mut parser = Parser::new(&c.commands);
    parser.know_projects(vec!["Homelab".to_string()]);
    for everyday in ["change the volume", "increase brightness", "add to my shopping list milk", "on the way home pick up bread", "fix the wobbly shelf"] {
        let i = parser.parse(everyday);
        println!("LIVE [improve] {everyday:?} -> {i:?}");
        assert!(!matches!(i, Intent::Improve(_)), "{everyday}: {i:?}");
    }
    for work in ["improve the date parsing", "on the Atlas project, add a date parser", "fix the parser in homelab", "refactor the tick loop"] {
        let i = parser.parse(work);
        println!("LIVE [improve] {work:?} -> {i:?}");
        assert!(matches!(i, Intent::Improve(_)), "{work}: {i:?}");
    }
}

/// The handlers read "tomorrow" off the wall clock, and between midnight and
/// 4 am "tomorrow" is asked about rather than booked (round 10). So a test
/// run in those hours (UTC, the shipped zone) is given a zone where it's
/// morning instead. Found at 00:44 UTC on 26 Sep, when every calendar test
/// that says "tomorrow" failed at once.
fn daytime(mut c: Config) -> Config {
    if (atlas::store::now() / 3600) % 24 < 5 {
        c.tools.as_mut().expect("the shipped config has tools").time_zone = "Asia/Shanghai".into();
    }
    c
}

// ============================ presence ====================================

#[test]
fn a_call_or_a_video_is_not_time_away() {
    let p = screen();
    let mut d = daemon(&p, "meeting");
    // The report reads "today" off the wall clock, so the fifty minutes this
    // drives must not straddle midnight: late in the day they're run earlier.
    // (It failed at 23:23 on 25 Sep, the call half in each day.)
    let now = atlas::store::now();
    let local_tod = d.home_zone().to_local(now as i64).rem_euclid(86_400);
    let t0 = if local_tod > 22 * 3600 { now - 2 * 3600 } else { now };
    let mut t = t0;
    *p.active.borrow_mut() = Some(win("Code.exe", "worklog.rs - atlas"));
    while t < t0 + 600 {
        *p.input_idle.borrow_mut() = Some(1);
        d.tick(t);
        t += 20;
    }
    // A 40-minute Zoom call: not a key pressed.
    *p.active.borrow_mut() = Some(win("Zoom.exe", "Zoom Meeting"));
    let quiet_from = t;
    while t < quiet_from + 2400 {
        *p.input_idle.borrow_mut() = Some(t - quiet_from);
        d.tick(t);
        t += 20;
    }
    *p.input_idle.borrow_mut() = Some(0);
    d.tick(t);
    let said = d.turn("what's outstanding", t + 5);
    println!("LIVE [meeting] after a 40-minute call: {said:?}");
    assert!(!said.contains("Before the break") && !said.contains("While you were away"), "{said}");
    let meeting: u64 = d.worklog.spans.iter().filter(|s| s.category == "meetings").map(|s| s.secs()).sum();
    println!("LIVE [meeting] recorded as meetings: {meeting} s");
    assert!(meeting >= 2300, "{meeting}");
    // Asked about one thing, answered about that thing.
    let asked = d.turn("how long was i on zoom", t + 10);
    println!("LIVE [meeting] you: \"how long was i on zoom\"  Atlas: {asked}");
    assert!(asked.contains("40 min") && asked.contains("zoom"), "{asked}");
}

#[test]
fn a_long_answer_is_atlas_busy_not_you_gone() {
    let p = screen();
    let mut d = daemon(&p, "stall");
    let t0 = atlas::store::now();
    let mut t = t0;
    *p.active.borrow_mut() = Some(win("Code.exe", "main.rs - atlas"));
    while t < t0 + 600 {
        *p.input_idle.borrow_mut() = Some(1);
        d.tick(t);
        t += 20;
    }
    // A 25-minute turn (a slow local model, a render): no tick ran, and you
    // were at the desk the whole time, moving the mouse.
    t += 1500;
    d.done_working(t);
    *p.input_idle.borrow_mut() = Some(2);
    d.tick(t + 1);
    let said = d.turn("thanks", t + 3);
    println!("LIVE [stall] after a 25-minute answer: {said:?}");
    assert!(!said.contains("Before the break"), "{said}");
}

#[test]
fn a_clock_set_back_does_not_break_the_report() {
    use atlas::worklog::{summarise, WorkLog, WorkLogConfig};
    let cfg = WorkLogConfig::default();
    let mut log = WorkLog::default();
    let w = win("Code.exe", "x");
    let mut t = 10_000u64;
    while t < 10_000 + 3000 {
        log.beat(&cfg, t, Some(&w), Some(0));
        t += 5;
    }
    // The clock jumps back twenty minutes.
    let mut t = 10_000 + 3000 - 1200;
    while t < 10_000 + 4000 {
        log.beat(&cfg, t, Some(&w), Some(0));
        t += 5;
    }
    assert!(log.spans.windows(2).all(|p| p[1].start >= p[0].end), "{:?}", log.spans);
    let s = summarise(&log.spans);
    println!("LIVE [clock] after the clock went back: {} s active, {} spans", s.active, log.spans.len());
    assert!(s.active <= 4000, "{}", s.active);
}

// ============================ held offers =================================

#[test]
fn the_bound_on_held_offers_fires_when_gates_ask_twice_a_tick() {
    use atlas::awareness::Signals;
    let cfg = ProactiveConfig { enabled: true, cooldown_secs: 0, max_interruptions_per_hour: 0, min_idle_secs: 0, ..Default::default() };
    let bound = cfg.defer_max_secs;
    let mut pro = Proactive::new(cfg);
    // Typing steadily for an hour, never a breakpoint; the daemon asks the
    // gate more than once each tick.
    let mut first_yes = None;
    let mut yeses = 0;
    for k in 0..(3600 / 10) {
        let t = 1_000_000 + k * 10;
        let s = Signals { input_idle_secs: Some(1), at_breakpoint: false, idle_secs: 999, ..Default::default() };
        let a = pro.may_interrupt(&s, t);
        let b = pro.may_interrupt(&s, t);
        assert_eq!(a, b, "one answer per tick");
        if a {
            yeses += 1;
            first_yes.get_or_insert(t - 1_000_000);
        }
    }
    println!("LIVE [bound] an hour of steady typing: first chance after {first_yes:?} s, {yeses} chances in the hour");
    let first = first_yes.expect("the bound lets an offer through");
    assert!(first >= bound && first <= bound + 10, "{first}");
    assert!((2..=4).contains(&yeses), "{yeses}");
}

#[test]
fn after_a_presentation_the_working_bound_starts_afresh() {
    use atlas::awareness::Signals;
    use atlas::platform::OsQuiet;
    let cfg = ProactiveConfig { enabled: true, cooldown_secs: 0, max_interruptions_per_hour: 0, min_idle_secs: 0, ..Default::default() };
    let mut pro = Proactive::new(cfg);
    let mut t = 5_000_000u64;
    // An hour presenting.
    for _ in 0..360 {
        let s = Signals { input_idle_secs: Some(1), os_quiet: Some(OsQuiet::Presenting), idle_secs: 999, ..Default::default() };
        assert!(!pro.may_interrupt(&s, t));
        t += 10;
    }
    // The first keystrokes after it are still typing, not an opening.
    let s = Signals { input_idle_secs: Some(1), os_quiet: Some(OsQuiet::Accepts), idle_secs: 999, ..Default::default() };
    assert!(!pro.may_interrupt(&s, t), "an offer would have landed on the first keystroke after presenting");
}

// ============================ crashes =====================================

#[test]
fn a_dotted_capital_i_does_not_end_the_session() {
    let p = screen();
    let mut d = daemon(&p, "dotted");
    let t = atlas::store::now();
    // Lower-casing "İ" adds a byte; a position found in the lower-cased text
    // used to cut the original inside a character.
    let said = d.turn("İlkay proposed tomorrow at 3 from Émile", t);
    println!("LIVE [unicode] {said:?}");
    assert!(!said.is_empty() && !said.contains("something went wrong"), "{said}");
    let said = d.turn("İstanbul team wants to meet tomorrow at 2 or friday at 10", t + 5);
    println!("LIVE [unicode] {said:?}");
    assert!(!said.is_empty() && !said.contains("something went wrong"), "{said}");
}

// ============================ learned breakpoints =========================

/// A synthetic day of pauses at one-second ticks, in three apps whose
/// ordinary pauses differ: the editor's are short, the trading terminal's
/// long (watching a chart is not a break), mail's short. Returns the ticks
/// as (t, app, idle) and every pause as (app, length, was it a real
/// boundary between sub-tasks).
fn pause_day(seed: u64) -> (Vec<(u64, &'static str, u64)>, Vec<(&'static str, u64, bool)>) {
    let mut x = seed;
    let mut rnd = |lo: u64, hi: u64| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        lo + x % (hi - lo + 1)
    };
    // (app, ordinary pause range, occasional longer ordinary pause, boundary range)
    let apps: [(&str, (u64, u64), (u64, u64), (u64, u64)); 3] = [
        ("Code.exe", (2, 12), (14, 26), (30, 75)),
        ("terminal64.exe", (5, 55), (55, 80), (100, 170)),
        ("OUTLOOK.EXE", (2, 9), (10, 16), (20, 45)),
    ];
    let mut ticks = Vec::new();
    let mut pauses = Vec::new();
    let mut t = 0u64;
    for round in 0..18 {
        let (app, ord, long, bound) = apps[round % 3];
        // A stint: a few sub-tasks, each some typing with ordinary pauses,
        // then a boundary pause.
        for _task in 0..rnd(3, 5) {
            for _ in 0..rnd(6, 12) {
                for _ in 0..rnd(5, 25) {
                    ticks.push((t, app, 0));
                    t += 1;
                }
                let p = if rnd(0, 9) == 0 { rnd(long.0, long.1) } else { rnd(ord.0, ord.1) };
                for k in 1..=p {
                    ticks.push((t, app, k));
                    t += 1;
                }
                pauses.push((app, p, false));
            }
            let p = rnd(bound.0, bound.1);
            for k in 1..=p {
                ticks.push((t, app, k));
                t += 1;
            }
            pauses.push((app, p, true));
        }
    }
    (ticks, pauses)
}

/// (precision, recall, false breaks) of "a pause at least this long is a
/// break", over one day's pauses.
fn judge(pauses: &[(&str, u64, bool)], threshold: &dyn Fn(&str) -> u64) -> (f64, f64, usize) {
    let (mut tp, mut fp, mut fnn) = (0, 0, 0);
    for (app, len, real) in pauses {
        let fired = *len >= threshold(app);
        match (fired, real) {
            (true, true) => tp += 1,
            (true, false) => fp += 1,
            (false, true) => fnn += 1,
            _ => {}
        }
    }
    (tp as f64 / (tp + fp).max(1) as f64, tp as f64 / (tp + fnn).max(1) as f64, fp)
}

#[test]
fn a_natural_pause_is_learned_per_app() {
    use atlas::worklog::WorkLog;
    // Learn on one day...
    let (ticks, _) = pause_day(0x5eed_0001);
    let mut log = WorkLog::default();
    for (t, app, idle) in &ticks {
        log.note_pause(app, *t, Some(*idle));
    }
    let learned = log.pause_thresholds();
    println!("LIVE [pauses] learned thresholds: {learned:?}");
    // ...judge on another.
    let (_, pauses) = pause_day(0x5eed_0002);
    let fixed = judge(&pauses, &|_| atlas::awareness::PAUSE);
    let mine = judge(&pauses, &|app| learned.get(app).copied().unwrap_or(atlas::awareness::PAUSE));
    println!(
        "LIVE [pauses] held-out day, {} pauses: fixed 20 s -> precision {:.2}, recall {:.2}, {} false breaks; learned per app -> precision {:.2}, recall {:.2}, {} false breaks",
        pauses.len(), fixed.0, fixed.1, fixed.2, mine.0, mine.1, mine.2
    );
    assert!(learned.len() == 3, "{learned:?}");
    assert!(learned["terminal64.exe"] > learned["Code.exe"], "{learned:?}");
    assert!(mine.2 * 2 <= fixed.2, "false breaks should at least halve: {} vs {}", mine.2, fixed.2);
    assert!(mine.1 >= 0.9, "recall {}", mine.1);
}

// ============================ a typed passphrase ==========================

#[test]
fn a_typed_passphrase_is_kept_off_the_screen_in_process_where_there_is_a_console() {
    // On Windows this really switches the console's echo off and back on.
    let (hidden, how) = atlas::typed::how_typing_is_hidden();
    println!("LIVE [typed] hidden={hidden}: {how}");
    assert!(hidden);
    if cfg!(windows) {
        assert!(how.contains("console") || how.contains("PowerShell"), "{how}");
    } else {
        assert!(how.contains("stty"), "{how}");
    }
}

// ============================ refining an animation =======================

const BALL: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100"><title>ball</title><rect width="200" height="100" fill="#ffffff"/><circle cx="20" cy="50" r="12" fill="#1e88e5" stroke="#1e88e5"><animate attributeName="cx" from="20" to="180" dur="2s" begin="0.5s" repeatCount="indefinite"/></circle><style>.pulse{animation: grow 3s ease-in-out infinite; animation-delay: 1s}</style></svg>"##;

#[test]
fn an_animation_is_refined_by_word_without_a_model() {
    use atlas::motion::refine;
    let r = refine(BALL, "make it twice as fast").unwrap();
    println!("LIVE [refine] \"make it twice as fast\": {:?}", r.changes);
    assert!(r.svg.contains("dur=\"1s\"") && r.svg.contains("begin=\"0.25s\"") && r.svg.contains("grow 1.5s") && r.svg.contains("animation-delay: 0.5s"), "{}", r.svg);
    assert_eq!(r.duration_secs, Some(1.5));
    let r = refine(BALL, "make it red").unwrap();
    println!("LIVE [refine] \"make it red\": {:?}", r.changes);
    assert!(r.svg.contains("fill=\"#e53935\"") && r.svg.contains("stroke=\"#e53935\"") && r.svg.contains("fill=\"#ffffff\""), "the background stays: {}", r.svg);
    let r = refine(BALL, "a bit slower and bigger").unwrap();
    println!("LIVE [refine] \"a bit slower and bigger\": {:?}", r.changes);
    assert_eq!(r.size, Some((300, 150)));
    assert!(r.svg.contains("viewBox=\"0 0 200 100\""), "{}", r.svg);
    assert!(refine(BALL, "make it lovely").is_none(), "nothing it can do: left to the rest of Atlas");
}

struct DrawsBall;
impl atlas::brain::Llm for DrawsBall {
    fn complete(&self, system: &str, _user: &str) -> atlas::error::Result<String> {
        if system == atlas::motion::MOTION_SYSTEM {
            return Ok(format!("```svg\n{BALL}\n```"));
        }
        Ok("{\"action\":\"say\",\"arg\":null,\"say\":\"?\"}".into())
    }
}

#[test]
fn make_it_faster_after_an_animation_edits_that_one() {
    let c: &'static Config = Box::leak(Box::new(Config::load(std::path::Path::new("config")).unwrap()));
    let p = screen();
    let mut d = Daemon::new(c, &p, Some(std::sync::Arc::new(DrawsBall)), Store::new(tmp("refine")), Proactive::new(ProactiveConfig::default()));
    let t = atlas::store::now();
    let first = d.turn("animate a ball rolling right, 200x100, for 3 seconds", t);
    println!("LIVE [refine] you: \"animate a ball rolling right…\"  Atlas: {}", first.replace('\n', " "));
    let said = d.turn("make it faster and red", t + 30);
    println!("LIVE [refine] you: \"make it faster and red\"  Atlas: {}", said.replace('\n', " "));
    assert!(said.starts_with("Made it faster") && said.contains("red") && said.contains(".v2.svg"), "{said}");
    // The file itself: the ball and its outline repainted, the time halved.
    let v2 = said.split("Saved to ").nth(1).and_then(|s| s.split(" (the one").next()).unwrap();
    let svg = std::fs::read_to_string(v2).unwrap();
    assert_eq!(svg.matches("#e53935").count(), 2, "{svg}");
    assert_eq!(atlas::motion::refine(&svg, "make it twice as fast").and_then(|r| r.duration_secs), Some(0.75));
    let said = d.turn("make it a bit slower", t + 60);
    println!("LIVE [refine] you: \"make it a bit slower\"  Atlas: {}", said.replace('\n', " "));
    assert!(said.contains(".v3.svg"), "{said}");
}

// ============================ adaptive sampling ===========================

fn scene_with(quality: &str, adaptive: bool, which: u8) -> atlas::scene3d::Scene {
    let body = if which == 0 {
        // Night, a soft moon and a lamp: noisy light (round 8's denoiser scene).
        r##""sky":"#0a0c12","sun":{"from":[-0.6,1,0.5],"softness":30,"strength":0.25},
          "camera":{"from":[0,1.4,4.5],"at":[0,0.5,0],"fov":40},
          "objects":[{"shape":"ground","colour":"#d8d0c0"},
                     {"shape":"sphere","radius":0.25,"at":[0.1,0.25,0.6],"material":"glow","colour":"#ffb050","glow":10},
                     {"shape":"sphere","radius":0.5,"at":[-0.7,0.5,0],"colour":"#d9730d"},
                     {"shape":"box","size":[0.6,0.6,0.6],"at":[0.9,0.3,-0.3],"rotate":[0,30,0],"colour":"#3060c0"}]"##
    } else {
        // Held out: daylight, hard edges, a checker and glass.
        r##""sky":"#9cc4ec","sun":{"from":[0.4,1,0.6],"softness":4},
          "camera":{"from":[0,1.2,4],"at":[0,0.4,0],"fov":42},
          "objects":[{"shape":"ground","colour":"#e0e0e0","pattern":"checker","colour2":"#303030","scale":0.5},
                     {"shape":"sphere","radius":0.45,"at":[-0.6,0.45,0],"material":"glass","colour":"#ffffff"},
                     {"shape":"box","size":[0.5,0.8,0.5],"at":[0.7,0.4,-0.2],"rotate":[0,20,0],"colour":"#c03030"}]"##
    };
    atlas::scene3d::parse_scene(&format!(
        r##"{{"width":160,"height":100,"quality":"{quality}","denoise":false,"adaptive":{adaptive},{body}}}"##
    ))
    .unwrap()
}

fn rmse(a: &atlas::pngcodec::Rgba, b: &atlas::pngcodec::Rgba) -> f64 {
    let n = a.pixels.len();
    let s: f64 = (0..n).filter(|i| i % 4 != 3).map(|i| (a.pixels[i] as f64 - b.pixels[i] as f64).powi(2)).sum();
    (s / (n as f64 * 0.75)).sqrt()
}

#[test]
fn more_rays_where_they_disagree_beats_more_rays_everywhere() {
    use atlas::scene3d::{render_frame, LAST_RAYS_PER_PIXEL_X1000};
    use std::sync::atomic::Ordering;
    let mut lines = Vec::new();
    for (which, name) in [(0u8, "night lamp"), (1u8, "held-out daylight")] {
        let reference = render_frame(&scene_with("reference", false, which), 0.0);
        let mut row = Vec::new();
        for (q, adaptive) in [("draft", false), ("draft", true), ("good", false), ("good", true), ("best", false)] {
            let img = render_frame(&scene_with(q, adaptive, which), 0.0);
            let spp = LAST_RAYS_PER_PIXEL_X1000.load(Ordering::Relaxed) as f64 / 1000.0;
            row.push((q, adaptive, spp, rmse(&img, &reference)));
        }
        let say: Vec<String> = row.iter().map(|(q, a, spp, e)| format!("{q}{} {spp:.1} rays/px → {e:.2}", if *a { "+adaptive" } else { "" })).collect();
        lines.push(format!("{name}: {}", say.join("; ")));
        let (draft, draft_a, good) = (row[0], row[1], row[2]);
        // Uniform error falls about as 1/sqrt(rays) between draft and good;
        // what would uniform sampling give at adaptive's cost?
        let k = (good.3 / draft.3).ln() / (good.2 / draft.2).ln();
        let uniform_at_cost = draft.3 * (draft_a.2 / draft.2).powf(k);
        lines.push(format!("{name}: uniform sampling at adaptive's {:.1} rays/px would be ≈{uniform_at_cost:.2}; adaptive {:.2}", draft_a.2, draft_a.3));
        if which == 0 {
            // Noisy light: a modest win at equal cost.
            assert!(draft_a.3 < uniform_at_cost, "{row:?}");
        } else {
            // Fine patterns: a loss, which is why it's off unless asked for.
            assert!(draft_a.3 > uniform_at_cost, "if this now wins, it can be turned on: {row:?}");
        }
    }
    assert_eq!(scene_with("draft", false, 0).adaptive, Some(false));

    for l in &lines {
        println!("LIVE [adaptive] {l}");
    }
}
