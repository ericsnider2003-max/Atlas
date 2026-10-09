#![allow(non_snake_case)]
//! Round 11's seventeen tools, driven through the front door: a sentence to
//! `Daemon::turn`, the real parser, the policy gate, the handler, the store.
//! A tool that works in its own tests and is reached wrongly from here is
//! the failure `every_intent_reaches_the_daemon` exists to catch.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::{ClipCopy, Grab, Monitor};
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

/// Now, to the minute. The handlers read the clock the way the daemon does
/// in use, so a turn's time and the handler's agree only near the real now.
fn now() -> u64 {
    let n = atlas::store::now();
    n - n % 60
}

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-workday-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn cfg() -> Config {
    daytime(Config::load(Path::new("config")).unwrap())
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


fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, dir: &Path) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(dir.to_path_buf()), Proactive::new(ProactiveConfig::default()))
}

#[test]
fn clipboard_history_is_off_until_turned_on_and_then_keeps_copies() {
    let T = now();
    let (c, p, dir) = (cfg(), plat(), tmp("clip"));
    let mut d = daemon(&c, &p, &dir);
    assert!(d.turn("clipboard history", T).contains("off"));
    assert!(d.turn("turn on clipboard history", T).contains("is on"));
    *p.clip_seq.borrow_mut() = Some(41);
    *p.clip_copy.borrow_mut() = Some(ClipCopy::Text("the Contoso invoice number".into()));
    p.focus_on("outlook", "Inbox");
    d.tick(T + 1);
    let said = d.turn("what did i copy", T + 5);
    assert!(said.contains("Contoso"), "{said}");
    p.set_clipboard("something else");
    let back = d.turn("paste the one 1", T + 6);
    assert!(back.contains("back on your clipboard"), "{back}");
    assert_eq!(p.clipboard_now().as_deref(), Some("the Contoso invoice number"));
    assert!(d.turn("turn off clipboard history", T + 7).contains("gone"));
}

#[test]
fn text_off_the_screen_lands_on_the_clipboard() {
    let T = now();
    let (c, p, dir) = (cfg(), plat(), tmp("screen"));
    *p.grab.borrow_mut() = Some(Grab { width: 2, height: 2, rgb: vec![0; 12], title: "Order".into() });
    *p.ocr.borrow_mut() = Some("Order 1182\nTotal\n$43.20".into());
    let mut d = daemon(&c, &p, &dir);
    let said = d.turn("copy the text off the screen", T);
    assert!(said.contains("on your clipboard"), "{said}");
    assert!(p.clipboard_now().unwrap().contains("$43.20"));
}

#[test]
fn the_market_calendar_answers_and_leans_nowhere() {
    let T = now();
    let (c, p, dir) = (cfg(), plat(), tmp("market"));
    let mut d = daemon(&c, &p, &dir);
    let said = d.turn("next market holiday", T);
    assert!(said.starts_with("Next: 20") && said.contains("US markets"), "{said}");
    assert!(!d.turn("is the market open", T).is_empty());
}

#[test]
fn waiting_for_reads_the_mail_cache_and_learns() {
    let T = now();
    let (c, p, dir) = (cfg(), plat(), tmp("waiting"));
    let mut book = atlas::mailbook::MailBook::default();
    book.letters.push(atlas::mailbook::Letter {
        id: "a@me".into(), in_reply_to: None, refs: vec![], from_name: String::new(), from: "me@x.com".into(),
        to: vec!["sam@y.com".into()], subject: "Q3 numbers".into(), at: T - 6 * 86_400, dated: true, mine: true,
        excerpt: "Could you send the Q3 numbers when you can?".into(),
    });
    Store::new(dir.clone()).save(atlas::mailbook::MailBook::FILE, &book).unwrap();
    let mut d = daemon(&c, &p, &dir);
    let said = d.turn("what am i waiting on", T);
    assert!(said.contains("Q3 numbers"), "{said}");
    // Owed: the brief lists it, then "done 1" closes it for good.
    let owed = |b: &atlas::brief::Brief| b.yours.iter().filter(|i| i.source == atlas::brief::Source::Day && i.subject.contains("Q3")).count();
    assert_eq!(owed(&d.brief_now(T + 1)), 1);
    d.turn("what am i waiting on", T + 2);
    assert!(d.turn("done 1", T + 3).contains("Closed"));
    assert!(!d.turn("what am i waiting on", T + 4).contains("Q3 numbers"));
    assert_eq!(owed(&d.brief_now(T + 5)), 0);
}

#[test]
fn a_dated_note_comes_up_in_its_day_and_a_review_lists_the_rest() {
    let T = now();
    let (c, p, dir) = (cfg(), plat(), tmp("notes"));
    let mut d = daemon(&c, &p, &dir);
    d.turn("note that call the bank tomorrow at 10", T);
    let due = d.notebook.notes.iter().find(|n| n.text.contains("bank")).and_then(|n| n.due).expect("dated");
    let utc = d.home_zone().to_utc(due as i64) as u64;
    let on_the_day = |d: &mut Daemon, at: u64| d.brief_now(at).yours.iter().any(|i| i.source == atlas::brief::Source::Day && i.subject.contains("bank"));
    assert!(on_the_day(&mut d, utc - 3600), "the morning of its day");
    assert!(!on_the_day(&mut d, utc - 2 * 86_400), "not two days before");
    assert!(!d.turn("review my notes", T).is_empty());
}

#[test]
fn an_unknown_app_name_is_looked_for_not_guessed() {
    let T = now();
    let (c, p, dir) = (cfg(), plat(), tmp("launch"));
    let mut d = daemon(&c, &p, &dir);
    assert!(d.turn("start up zzqxv", T).contains("can't find"));
}

#[test]
fn the_trading_check_in_is_asked_answered_and_counted() {
    let T = now();
    let (c, p, dir) = (cfg(), plat(), tmp("trade"));
    let mut d = daemon(&c, &p, &dir);
    let asked = d.turn("trading check in", T);
    assert!(asked.contains("1. ") && asked.ends_with(atlas::tradeday::THE_LINE), "{asked}");
    assert!(d.turn("yes yes no yes 4", T + 10).contains("Noted"));
    let s = d.turn("how has my trading process been", T + 20);
    assert!(s.contains("checked in before 1"), "{s}");
    assert!(s.ends_with(atlas::tradeday::THE_LINE));
}

#[test]
fn meeting_prep_names_the_people_and_what_is_open() {
    let T = now();
    let (c, p, dir) = (cfg(), plat(), tmp("meeting"));
    let mut d = daemon(&c, &p, &dir);
    assert!(d.turn("prep me for my next meeting", T).contains("Nothing on your calendar"));
    d.calendar.add("Call with Sam Lee", atlas::calendar::When { start: T + 600, end: T + 4200, all_day: false }, None, T);
    let said = d.turn("prep me for my next meeting", T);
    let lines: Vec<&str> = said.lines().collect();
    assert_eq!(lines.len(), 2, "the title, then one line for Sam: {said}");
    assert!(lines[1].starts_with("Sam Lee: no mail"), "{said}");
    // Offered on its own at most once, inside the prep window.
    let offered: usize = (0..4).map(|k| d.tick(T + 120 + k * 61).iter().filter(|l| l.contains("Call with Sam Lee")).count()).sum();
    assert!(offered <= 1, "said {offered} times");
}

#[test]
fn unavailable_calendar_is_not_an_empty_day_or_a_first_run() {
    let t = now();
    let (c, p, dir) = (cfg(), plat(), tmp("calendar-unavailable"));
    let store = Store::new(&dir);
    std::fs::write(store.root().join("calendar.json"), b"{broken").unwrap();
    let mut d = daemon(&c, &p, &dir);
    assert!(d.calendar.availability_error().is_some());
    let said = d.turn("prep me for my next meeting", t);
    assert!(said.contains("calendar is unavailable") && !said.contains("Nothing on your calendar"), "{said}");
    // Even a retained in-memory event is not offered as current evidence.
    d.calendar.add("Unconfirmed cached call with Sam Lee", atlas::calendar::When { start: t + 600, end: t + 4200, all_day: false }, None, t);
    let deck = d.deck(t, 0);
    assert!(deck.now_sub.contains("Calendar unavailable"));
    assert!(!deck.first_run);
    assert!(deck.spine.iter().all(|(_, title, _)| !title.contains("Unconfirmed cached call")));
    assert!(d.tick(t + 120).iter().all(|line| !line.contains("Unconfirmed cached call")));
}

#[test]
fn a_snippet_is_saved_and_typed_into_the_app_in_front() {
    let T = now();
    let (c, p, dir) = (cfg(), plat(), tmp("snippet"));
    let mut d = daemon(&c, &p, &dir);
    assert!(d.turn("save snippet ;sig as Best, Eric", T).contains("Saved"));
    p.focus_on("notepad", "Untitled");
    let said = d.turn("type my sig", T);
    assert!(said.contains("Typed"), "{said}");
    assert!(p.typed().iter().any(|t| t == "Best, Eric"));
    assert!(d.turn("my snippets", T).contains(";sig"));
}

fn entry(path: &str, modified: u64) -> atlas::index::Entry {
    let name = Path::new(path).file_name().unwrap().to_string_lossy().to_string();
    let ext = name.rsplit_once('.').map(|x| x.1.to_string()).unwrap_or_default();
    atlas::index::Entry { path: path.into(), name, ext, size: 1, modified, class: atlas::index::AssetClass::Other }
}

#[test]
fn a_file_is_found_opened_by_number_and_pdfs_merged_beside_it() {
    let T = now();
    let (c, p, dir) = (cfg(), plat(), tmp("files"));
    let docs = dir.join("docs");
    std::fs::create_dir_all(&docs).unwrap();
    let fixture = std::fs::canonicalize("tests/fixtures/round11/three_pages.pdf").unwrap();
    let a = docs.join("report-a.pdf");
    let b = docs.join("report-b.pdf");
    std::fs::copy(&fixture, &a).unwrap();
    std::fs::copy(&fixture, &b).unwrap();
    let mut d = daemon(&c, &p, &dir);
    for f in [&a, &b] {
        let s = f.to_string_lossy().to_string();
        d.index.entries.insert(s.clone(), entry(&s, T - 100));
    }
    let found = d.turn("find the pdf report", T);
    assert!(found.contains("1.") && found.contains("2."), "{found}");
    assert!(d.turn("open 1", T + 1).contains("Opening"));
    assert!(p.typed().iter().any(|t| t.starts_with("open:") && t.ends_with(".pdf")));
    let merged = d.turn("merge the pdfs 1 and 2", T + 2);
    assert!(merged.contains("merged") && merged.contains("6 pages"), "{merged}");
    assert!(docs.join("report-a (merged).pdf").exists());
    // Again: a new name, never over the first.
    d.turn("find the pdf report-a", T + 3);
    let again = d.turn("merge the pdfs 1 and 2", T + 4);
    assert!(again.contains("Merging needs two") || docs.join("report-a (merged 2).pdf").exists(), "{again}");
    // Eleven minutes on, "open 1" is an ordinary sentence again.
    let late = d.turn("open 1", T + 700);
    assert!(!late.starts_with("Opening report"), "{late}");
}

#[test]
fn people_are_kept_by_naming_them() {
    let T = now();
    let (c, p, dir) = (cfg(), plat(), tmp("people"));
    let mut d = daemon(&c, &p, &dir);
    assert!(d.turn("keep in touch with Priya every 3 weeks", T).contains("21 days"));
    assert!(d.turn("remember Priya's daughter is called Leo", T).contains("Noted"));
    let about = d.turn("what do I know about Priya", T);
    assert!(about.contains("Leo"), "{about}");
    assert!(d.turn("who should I catch up with", T).contains("Priya"));
    // Saved: a new daemon on the same store knows her.
    let mut d2 = daemon(&c, &p, &dir);
    assert!(d2.turn("what do I know about Priya", T).contains("Leo"));
}

#[test]
fn feeds_say_so_when_nothing_is_followed() {
    let T = now();
    let (c, p, dir) = (cfg(), plat(), tmp("feeds"));
    let mut d = daemon(&c, &p, &dir);
    assert!(d.turn("my feeds", T).contains("not following"));
}

#[test]
fn a_receipt_is_kept_from_the_clipboard_and_summed() {
    let T = now();
    let (c, p, dir) = (cfg(), plat(), tmp("receipt"));
    let mut d = daemon(&c, &p, &dir);
    p.set_clipboard("COSTCO WHOLESALE\n09/24/2026\nSUBTOTAL 11.48\nTAX 0.95\nTOTAL 12.43");
    let kept = d.turn("keep this receipt", T);
    assert!(kept.contains("$12.43"), "{kept}");
    assert!(d.turn("what did i spend at costco", T).contains("$12.43"));
    // An unlabelled one is asked about, and "yes" keeps it.
    p.set_clipboard("Corner Cafe total\nlatte 4.50\nmuffin 3.25\n7.75");
    let asked = d.turn("keep this receipt", T + 60);
    if asked.contains("probably") {
        assert!(d.turn("yes", T + 61).contains("Kept"));
    }
}

#[test]
fn habits_are_made_ticked_and_shown() {
    let T = now();
    let (c, p, dir) = (cfg(), plat(), tmp("habits"));
    let mut d = daemon(&c, &p, &dir);
    assert!(d.turn("new habit: stretch daily", T).contains("Tracking"));
    assert!(d.turn("did my stretch", T).contains("done"));
    assert!(d.turn("how are my habits", T).to_lowercase().contains("stretch (daily)"));
}

#[test]
fn cards_are_made_and_quizzed() {
    let T = now();
    let (c, p, dir) = (cfg(), plat(), tmp("cards"));
    let mut d = daemon(&c, &p, &dir);
    assert!(d.turn("make a card: capital of Peru | Lima", T).contains("Card made"));
    assert!(d.turn("quiz me", T).to_lowercase().contains("capital of peru"));
    assert!(d.turn("show", T).contains("Lima"));
    let graded = d.turn("good", T);
    assert!(graded.contains("Back in 3 days"), "{graded}");
}

#[test]
fn translation_says_plainly_when_there_is_no_model() {
    let T = now();
    let (c, p, dir) = (cfg(), plat(), tmp("translate"));
    let mut d = daemon(&c, &p, &dir);
    assert!(d.turn("translate good morning to French", T).contains("local model"));
}

#[test]
fn a_sentence_about_someone_unknown_is_not_taken() {
    let T = now();
    // "I called the bank" is not a person to file.
    let (c, p, dir) = (cfg(), plat(), tmp("unknown"));
    let mut d = daemon(&c, &p, &dir);
    let _ = d.turn("I called the bank about the card", T);
    let people: atlas::people::People = Store::new(dir.clone()).load("people");
    assert!(people.by_key.is_empty());
}
