//! Your social accounts and the people you watch (`social`, 29 Sep 2026).
//!
//! Real behaviour against real-shaped data: the importers read synthetic
//! exports built to the platforms' published formats, the feed readers read
//! answers saved from the live services, and the API readers are driven
//! through a stand-in network that answers with those same shapes (see
//! `tests/fixtures/social/FORMATS.md`). The rule every test here leans on:
//! a number in an answer is one that came out of the data.

use atlas::social::analysis;
use atlas::social::apis::{self, Net, Reply};
use atlas::social::exports;
use atlas::social::onepage;
use atlas::social::snapshots::{AccountSnap, Book, Metrics, Platform, PostSnap, Record};
use atlas::social::watchlist::{self, Failed, Fetched, Quota, Target, Watch};
use atlas::social::SocialConfig;
use std::cell::RefCell;
use std::path::{Path, PathBuf};

/// 28 Sep 2026, noon UTC.
const NOW: u64 = 1_790_596_800;
const DAY: u64 = 86_400;

fn fx(p: &str) -> PathBuf {
    Path::new("tests/fixtures/social").join(p)
}

fn text(p: &str) -> String {
    std::fs::read_to_string(fx(p)).unwrap()
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-social-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn posts(recs: &[Record]) -> Vec<&PostSnap> {
    recs.iter().filter_map(|r| if let Record::Post(p) = r { Some(p) } else { None }).collect()
}

fn accounts(recs: &[Record]) -> Vec<&AccountSnap> {
    recs.iter().filter_map(|r| if let Record::Account(a) = r { Some(a) } else { None }).collect()
}

// ---------------------------------------------------------------- the record

fn post(pf: Platform, id: &str, day_posted: u64, day: i64, views: Option<u64>, text: &str, secs: Option<f64>) -> Record {
    Record::Post(PostSnap {
        platform: pf,
        id: id.into(),
        day,
        taken: NOW,
        posted: Some(day_posted),
        text: text.into(),
        url: String::new(),
        seconds: secs,
        source: "test".into(),
        m: Metrics { views, ..Default::default() },
    })
}

#[test]
fn the_record_is_appended_once_a_day_and_the_later_numbers_win() {
    let dir = tmp("record");
    let path = dir.join(atlas::social::snapshots::FILE);
    let mut b = Book::load(&path);
    assert!(b.is_empty(), "no file is an empty record, not an error");
    let day = atlas::social::social_day(NOW);
    let a = b.add(&path, vec![post(Platform::Youtube, "v1", NOW - DAY, day, Some(100), "x", None)]).unwrap();
    assert_eq!((a.new, a.unchanged), (1, 0));
    // The same numbers again the same day: not written twice.
    let a = b.add(&path, vec![post(Platform::Youtube, "v1", NOW - DAY, day, Some(100), "x", None)]).unwrap();
    assert_eq!((a.new, a.unchanged), (0, 1));
    assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 1);
    // New numbers the same day: written, and they are what's read back.
    b.add(&path, vec![post(Platform::Youtube, "v1", NOW - DAY, day, Some(150), "x", None)]).unwrap();
    b.add(&path, vec![post(Platform::Youtube, "v1", NOW - DAY, day + 1, Some(400), "x", None)]).unwrap();
    // A torn last line, as a crash mid-write leaves.
    use std::io::Write;
    std::fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(b"{\"kind\":\"post\",\"platf").unwrap();
    let again = Book::load(&path);
    assert_eq!(again.unreadable, 1, "the torn line is counted, not fatal");
    let h = again.history(Platform::Youtube, "v1");
    assert_eq!(h.iter().map(|p| p.m.views).collect::<Vec<_>>(), vec![Some(150), Some(400)], "one per day, the later numbers for a day");
    assert_eq!(again.latest_posts()[0].m.views, Some(400));
    assert_eq!(again.len(), 2);
}

#[test]
fn a_missing_number_stays_missing_and_is_never_written_as_zero() {
    let m = Metrics { likes: Some(3), ..Default::default() };
    let json = serde_json::to_string(&m).unwrap();
    assert_eq!(json, "{\"likes\":3}", "absent figures aren't written at all");
    assert_eq!(Metrics::default().interactions(), None);
    assert_eq!(m.interactions(), Some(3));
    let mut a = Metrics { views: Some(10), ..Default::default() };
    a.fill_from(&Metrics { views: Some(99), avg_view_pct: Some(41.5), ..Default::default() });
    assert_eq!((a.views, a.avg_view_pct), (Some(10), Some(41.5)), "fill keeps what's there and adds what isn't");
}

// ---------------------------------------------------------------- the exports

#[test]
fn an_x_archive_folder_reads_its_posts_and_leaves_out_reposts() {
    let got = exports::import_path(&fx("exports/x_archive"), NOW).unwrap();
    assert_eq!(got.platform, Platform::X);
    assert_eq!(got.handle, "jordanedits");
    let p = posts(&got.records);
    assert_eq!(p.len(), 4, "five in the file, one a repost of someone else");
    let stop = p.iter().find(|x| x.text.starts_with("Stop exporting")).unwrap();
    assert_eq!((stop.m.likes, stop.m.reposts, stop.m.views, stop.m.impressions), (Some(120), Some(33), None, None));
    assert_eq!(stop.url, "https://x.com/jordanedits/status/1839000000000000002");
    // "Tue Sep 09 18:30:00 +0000 2026"
    assert_eq!(stop.posted, Some(atlas::civil::days_from_civil(2026, 9, 9) as u64 * DAY + 18 * 3600 + 30 * 60));
    // As of the archive's own generation date, not the day it was imported.
    assert_eq!(stop.day, atlas::civil::days_from_civil(2026, 9, 27));
    let a = accounts(&got.records);
    assert_eq!(a[0].followers, Some(7));
    assert!(got.missing.iter().any(|m| m.contains("impressions") && m.contains("Premium")), "{:?}", got.missing);
    assert!(got.missing.iter().any(|m| m.contains("1 reposts")), "{:?}", got.missing);
}

#[test]
fn the_same_archive_zipped_reads_the_same() {
    let dir = exports::import_path(&fx("exports/x_archive"), NOW).unwrap();
    let zip = exports::import_path(&fx("exports/x_archive.zip"), NOW).unwrap();
    assert_eq!(posts(&dir.records).len(), posts(&zip.records).len());
    assert_eq!(posts(&dir.records)[0].m, posts(&zip.records)[0].m);
}

#[test]
fn a_tiktok_export_has_videos_and_followers_and_says_it_has_no_views() {
    let got = exports::import_path(&fx("exports/tiktok"), NOW).unwrap();
    assert_eq!(got.platform, Platform::Tiktok);
    assert_eq!(got.handle, "jordan.makes");
    let p = posts(&got.records);
    assert_eq!(p.len(), 2);
    let v = p.iter().find(|x| x.id == "7418000000000000001").unwrap();
    assert_eq!((v.m.likes, v.m.views), (Some(120), None));
    assert_eq!(v.text, "Stop editing like this");
    assert_eq!(accounts(&got.records)[0].followers, Some(3));
    assert!(got.missing.iter().any(|m| m.contains("views per video")));
    assert!(got.missing.iter().any(|m| m.contains("retention")));
}

#[test]
fn an_instagram_download_joins_posts_to_their_insights_and_repairs_the_text() {
    let got = exports::import_path(&fx("exports/instagram"), NOW).unwrap();
    assert_eq!(got.platform, Platform::Instagram);
    let p = posts(&got.records);
    assert_eq!(p.len(), 3, "two posts and a reel");
    let desk = p.iter().find(|x| x.text.contains("finished desk")).unwrap();
    assert_eq!(desk.text, "Here\u{2019}s the finished desk", "Meta's Latin-1 escaping undone");
    assert_eq!((desk.m.impressions, desk.m.reach, desk.m.likes, desk.m.saves), (Some(1204), Some(980), Some(88), Some(14)));
    let reel = p.iter().find(|x| x.text.starts_with("If you edit")).unwrap();
    assert_eq!(reel.m.views, Some(5310));
    let carousel = p.iter().find(|x| x.text == "Three cables, one hub").unwrap();
    assert_eq!(carousel.m, Metrics::default(), "no insights for it, so no figures -- not zeros");
    assert_eq!(accounts(&got.records)[0].followers, Some(4));
    assert_eq!(exports::meta_text("plain"), "plain");
}

#[test]
fn a_linkedin_export_reads_days_followers_and_both_top_post_tables() {
    let got = exports::import_path(&fx("exports/Content_2026-09-01_2026-09-28_JordanLee.xlsx"), NOW).unwrap();
    assert_eq!(got.platform, Platform::Linkedin);
    assert_eq!(got.handle, "JordanLee");
    let d28 = atlas::civil::days_from_civil(2026, 9, 28);
    let a = accounts(&got.records);
    let last = a.iter().find(|x| x.day == d28).unwrap();
    assert_eq!(last.followers, Some(1532));
    assert_eq!((last.m.impressions, last.m.engagements, last.m.followers_gained), (Some(405), Some(18), Some(5)), "Excel dates and string dates meet on the same day");
    let p = posts(&got.records);
    assert_eq!(p.len(), 2);
    let one = p.iter().find(|x| x.id == "7240000000000000001").unwrap();
    assert_eq!((one.m.engagements, one.m.impressions), (Some(44), Some(1502)), "joined across the two side-by-side tables");
    assert_eq!(one.posted, Some(atlas::civil::days_from_civil(2026, 9, 24) as u64 * DAY));
}

#[test]
fn a_youtube_studio_table_skips_its_total_and_reads_retention() {
    let got = exports::import_path(&fx("exports/youtube_studio"), NOW).unwrap();
    assert_eq!(got.platform, Platform::Youtube);
    let p = posts(&got.records);
    assert_eq!(p.len(), 3, "the Total row is not a video");
    let a = p.iter().find(|x| x.id == "aB3dE5gH7jK").unwrap();
    assert_eq!(a.m.views, Some(9120));
    assert_eq!(a.m.avg_view_secs, Some(27.0));
    assert_eq!(a.m.avg_view_pct, Some(65.85));
    assert_eq!(a.seconds, Some(41.0));
    assert!((a.m.watch_minutes.unwrap() - 251.4 * 60.0).abs() < 1e-6);
    assert_eq!(a.posted, Some(atlas::civil::days_from_civil(2026, 9, 3) as u64 * DAY));
    let quoted = p.iter().find(|x| x.id == "Zx9Yw8Vu7Ts").unwrap();
    assert_eq!(quoted.text, "My editing desk, \"finished\"");
    assert!(a.source.contains("date range on screen"), "the figures cover the range that was shown, and say so");
}

#[test]
fn something_that_is_not_an_export_is_named_as_that() {
    let dir = tmp("notexport");
    std::fs::write(dir.join("notes.txt"), "hello").unwrap();
    let e = exports::import_path(&dir, NOW).unwrap_err();
    assert!(e.contains("don't recognise"), "{e}");
    assert!(exports::import_path(&dir.join("missing"), NOW).unwrap_err().contains("can't open"));
}

// ---------------------------------------------------------------- the spreadsheet reader

#[test]
fn the_spreadsheet_reader_reads_shared_inline_rich_and_gapped_cells() {
    let bytes = std::fs::read(fx("handmade.xlsx")).unwrap();
    let sheets = atlas::social::xlsx::sheets(&bytes).unwrap();
    assert_eq!(sheets.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), vec!["Numbers & words", "Second"], "the workbook's order, whatever the relationships' order");
    let s = &sheets[0];
    assert_eq!(s.rows[0], vec!["Date", "", "Rich text <here>"]);
    assert!(s.rows[1].is_empty(), "the skipped row keeps its place");
    assert_eq!(s.rows[2], vec!["46293", "inline", "TRUE", "a formula's text"]);
    assert_eq!(atlas::social::xlsx::excel_day(46293.0), atlas::civil::days_from_civil(2026, 9, 28));
    assert_eq!(sheets[1].rows[0].len(), 28, "AB is the 28th column");
    assert_eq!(sheets[1].rows[0][27], "2.5");
    assert!(atlas::social::xlsx::sheets(b"not a zip").is_err());
}

#[test]
fn csv_rows_handle_quotes_commas_and_a_bom() {
    let rows = exports::csv_rows("\u{feff}a,\"b, c\",\"say \"\"hi\"\"\"\r\n1,2,3\n");
    assert_eq!(rows, vec![vec!["a", "b, c", "say \"hi\""], vec!["1", "2", "3"]]);
}

// ---------------------------------------------------------------- what the numbers say

fn book_of(recs: Vec<Record>) -> Book {
    let dir = tmp(&format!("book-{}", recs.len()));
    let path = dir.join("b.jsonl");
    let mut b = Book::load(&path);
    b.add(&path, recs).unwrap();
    b
}

#[test]
fn your_last_video_is_compared_at_the_same_age_when_the_record_has_it() {
    let today = atlas::social::social_day(NOW);
    let mut recs = Vec::new();
    // Four earlier videos, each seen two days after it went out.
    for (i, v) in [100u64, 200, 300, 400].iter().enumerate() {
        let posted = NOW - (20 - i as u64 * 3) * DAY;
        let d0 = atlas::social::social_day(posted);
        recs.push(post(Platform::Youtube, &format!("old{i}"), posted, d0 + 2, Some(*v), "Here's the thing", Some(40.0)));
        recs.push(post(Platform::Youtube, &format!("old{i}"), posted, today, Some(*v * 5), "Here's the thing", Some(40.0)));
    }
    // The latest, two days old.
    recs.push(post(Platform::Youtube, "new", NOW - 2 * DAY, today - 1, Some(500), "Stop doing this", Some(30.0)));
    recs.push(post(Platform::Youtube, "new", NOW - 2 * DAY, today, Some(750), "Stop doing this", Some(30.0)));
    let b = book_of(recs);
    let said = analysis::last_video(&b, None, NOW);
    assert!(said.contains("\"Stop doing this\""), "{said}");
    assert!(said.contains("750 views"), "{said}");
    assert!(said.contains("Up 250 views"), "{said}");
    assert!(said.contains("median of 250 views at the same age"), "{said}");
    assert!(said.contains("3.0x"), "{said}");
    // Every figure it said is in the data or worked out from it: the views
    // (750), the gain and the median (250), the ratio (3.0), how many it
    // compared (4), and the days (26, 27, 28 Sep).
    assert!(analysis::grounded(&said, "750 250 3.0 4 26 27 28"), "{said}");
}

#[test]
fn with_no_videos_it_says_what_to_do_not_zero() {
    let b = Book::default();
    let said = analysis::last_video(&b, None, NOW);
    assert!(said.contains("no videos"), "{said}");
    assert!(!said.contains('0'), "no number where there's no data: {said}");
    let said = analysis::what_worked(&b, None, 30, NOW, 8);
    assert!(said.contains("Nothing you posted"), "{said}");
    // 29 Sep 2026: Threads and Facebook Pages are read now, so they're named too.
    assert!(said.contains("Nothing from YouTube, TikTok, Instagram, X, LinkedIn, Bluesky, Threads and Facebook yet"), "{said}");
    assert!(analysis::followers(&b, None, NOW).contains("no follower counts"));
}

#[test]
fn under_the_pattern_floor_it_gives_numbers_and_calls_patterns_noise() {
    let recs = (0..5).map(|i| post(Platform::Tiktok, &format!("t{i}"), NOW - (i + 1) * DAY, atlas::social::social_day(NOW), Some(100 * (i + 1)), "a video", None)).collect();
    let said = analysis::what_worked(&book_of(recs), None, 30, NOW, 8);
    assert!(said.contains("5 posts on record"), "{said}");
    assert!(said.contains("would be noise"), "{said}");
    assert!(said.contains("TikTok and X don't give them"), "{said}");
}

#[test]
fn over_the_floor_it_names_the_opening_length_and_retention_that_did_best() {
    let today = atlas::social::social_day(NOW);
    let mut recs = Vec::new();
    for i in 0..12u64 {
        let good = i < 4;
        let mut r = post(
            Platform::Youtube,
            &format!("v{i}"),
            NOW - (i + 1) * DAY,
            today,
            Some(if good { 5000 + i } else { 1000 + i }),
            if good { "Stop using presets for this" } else { "In this video I explain my process" },
            Some(if good { 30.0 } else { 90.0 }),
        );
        if let Record::Post(p) = &mut r {
            p.m.avg_view_pct = Some(if good { 70.0 } else { 30.0 });
        }
        recs.push(r);
    }
    let said = analysis::what_worked(&book_of(recs), Some(Platform::Youtube), 30, NOW, 8);
    assert!(said.contains("4 of your top 4 open with one that says something that sounds wrong"), "{said}");
    assert!(said.contains("against 0 of the other 8"), "{said}");
    assert!(said.contains("the best ran a median 30s, the rest 90s"), "{said}");
    assert!(said.contains("70% through on average, the rest 30%"), "{said}");
}

#[test]
fn posting_times_need_enough_posts_in_each_slot() {
    let zone = atlas::tz::Zone::utc();
    let today = atlas::social::social_day(NOW);
    let day0 = today as u64 * DAY;
    let mut recs = Vec::new();
    // Mornings do twice as well as evenings.
    for i in 0..4u64 {
        recs.push(post(Platform::Youtube, &format!("m{i}"), day0 - (i + 1) * DAY + 10 * 3600, today, Some(2000), "x", None));
        recs.push(post(Platform::Youtube, &format!("e{i}"), day0 - (i + 1) * DAY + 20 * 3600, today, Some(1000), "x", None));
    }
    recs.push(post(Platform::Youtube, "late", day0 - 2 * DAY + 3 * 3600, today, Some(1500), "x", None));
    let said = analysis::posting_times(&book_of(recs), None, &zone, 3);
    assert!(said.contains("best morning (9am-noon)"), "{said}");
    assert!(said.contains("weakest evening (6pm-midnight)"), "{said}");
    assert!(said.contains("Too few posts to judge overnight"), "{said}");
    assert!(said.contains("not proof"), "{said}");
    let few = analysis::posting_times(&Book::default(), None, &zone, 3);
    assert!(few.contains("too few"), "{few}");
}

#[test]
fn followers_over_time_per_platform_and_together_naming_the_gaps() {
    let today = atlas::social::social_day(NOW);
    let acct = |pf: Platform, day: i64, n: u64| {
        Record::Account(AccountSnap { platform: pf, handle: "me".into(), day, taken: NOW, followers: Some(n), following: None, posts: None, source: "t".into(), m: Metrics::default() })
    };
    let b = book_of(vec![acct(Platform::Youtube, today - 30, 1000), acct(Platform::Youtube, today - 7, 1100), acct(Platform::Youtube, today, 1180), acct(Platform::Bluesky, today, 40)]);
    let said = analysis::followers(&b, None, NOW);
    assert!(said.contains("YouTube: 1,180"), "{said}");
    assert!(said.contains("+80 over the week"), "{said}");
    assert!(said.contains("+180 over the 30 days"), "{said}");
    assert!(said.contains("Bluesky: 40") && said.contains("one day on record"), "{said}");
    assert!(said.contains("Together: 1,220 followers"), "{said}");
    // 29 Sep 2026: Threads and Facebook joined the platforms named.
    assert!(said.contains("No follower figures from TikTok, Instagram, X, LinkedIn, Threads and Facebook"), "{said}");
}

#[test]
fn the_brief_mentions_a_video_only_while_it_is_new() {
    let today = atlas::social::social_day(NOW);
    let fresh = book_of(vec![post(Platform::Tiktok, "t1", NOW - DAY, today, Some(1234), "x", None)]);
    assert_eq!(analysis::own_brief_line(&fresh, NOW).as_deref(), Some("Your latest TikTok video: 1,234 views so far"));
    let old = book_of(vec![post(Platform::Tiktok, "t1", NOW - 5 * DAY, today, Some(1234), "x", None)]);
    assert_eq!(analysis::own_brief_line(&old, NOW), None);
    let no_views = book_of(vec![post(Platform::Tiktok, "t1", NOW - DAY, today, None, "x", None)]);
    assert_eq!(analysis::own_brief_line(&no_views, NOW), None, "no figure, no line -- never a zero");
}

#[test]
fn a_model_summary_with_a_number_of_its_own_is_not_used() {
    let facts = "On Hacker News\n- \"Sonnet 5.5\" -- 835 points, 564 comments\n- a video: 1.9M views, 2.0x the usual pace";
    assert!(analysis::grounded("Sonnet 5.5 drew 835 points and 564 comments.", facts));
    assert!(analysis::grounded("It has 1.9 million views, about 2x its usual.", facts));
    assert!(!analysis::grounded("It drew 900 points.", facts), "900 is nowhere in the facts");
    assert!(!analysis::grounded("Up 40% this week.", facts));
    assert!(analysis::grounded("Nothing numeric here.", facts));
}

// ---------------------------------------------------------------- the feeds, as they really answer

#[test]
fn a_youtube_channel_feed_gives_views_and_likes_without_a_key() {
    let items = watchlist::parse_youtube_feed(&text("youtube_channel.xml")).unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].id, "rayrrXot17M");
    assert_eq!(items[0].views, Some(1_900_154));
    assert_eq!(items[0].likes, Some(34_326));
    assert_eq!(items[0].who, "Marques Brownlee");
    assert!(items[0].at.is_some());
    assert!(items.iter().all(|i| i.views.is_some()));
    assert_eq!(watchlist::channel_id_in_page("<link rel=\"canonical\" href=\"https://www.youtube.com/channel/UCBJycsmduvYEL83R_U4JriQ\">"), Some("UCBJycsmduvYEL83R_U4JriQ".into()));
}

#[test]
fn trends_hn_mastodon_bluesky_and_product_hunt_read_as_they_answer() {
    let tr = watchlist::parse_trends(&text("google_trends.xml")).unwrap();
    assert_eq!(tr.len(), 3);
    assert_eq!(tr[0].title, "hbo harry potter series cast");
    assert_eq!(tr[0].traffic.as_deref(), Some("500+"));
    let hn = watchlist::parse_hn(&text("hn_front_page.json")).unwrap();
    assert_eq!((hn[0].title.as_str(), hn[0].points, hn[0].comments), ("Sonnet 5.5", Some(835), Some(564)));
    let m = watchlist::parse_mastodon(&text("mastodon_tag.json")).unwrap();
    assert_eq!(m.len(), 2);
    assert!(m.iter().all(|s| s.likes.is_some() && s.reposts.is_some() && !s.url.is_empty()));
    let b = watchlist::parse_bluesky(&text("bluesky_author_feed.json")).unwrap();
    assert!(b.iter().any(|s| s.likes == Some(755) && s.reposts == Some(92)), "{b:?}");
    let ph = watchlist::parse_plain_feed(&text("producthunt.xml")).unwrap();
    assert_eq!(ph.len(), 3);
    assert!(ph.iter().all(|s| s.views.is_none() && s.likes.is_none()), "the feed has no votes and none are made up");
    assert!(watchlist::parse_mastodon("<html>This page is not correct - Mastodon</html>").unwrap_err().contains("didn't answer with posts"));
}

#[test]
fn bluesky_own_account_reads_profile_and_posts_through_the_api_reader() {
    struct Saved;
    impl Net for Saved {
        fn get(&self, _h: &str, path: &str, _hd: &[(&str, &str)]) -> Result<Reply, String> {
            let f = if path.contains("getProfile") { "bluesky_profile.json" } else { "bluesky_author_feed.json" };
            Ok(Reply { status: 200, body: text(f), ..Default::default() })
        }
        fn post_form(&self, _: &str, _: &str, _: &str) -> Result<Reply, String> {
            unreachable!()
        }
        fn post_json(&self, _: &str, _: &str, _: &[(&str, &str)], _: &str) -> Result<Reply, String> {
            unreachable!()
        }
    }
    let recs = apis::bluesky_own(&Saved, "bsky.app", NOW).unwrap();
    let a = accounts(&recs)[0];
    assert_eq!(a.handle, "bsky.app");
    assert!(a.followers.unwrap() > 1000);
    let p = posts(&recs);
    assert!(!p.is_empty());
    assert!(p.iter().all(|x| x.m.views.is_none()), "Bluesky has no views, and none are invented");
    assert!(p.iter().any(|x| x.m.likes == Some(755)));
}

// ---------------------------------------------------------------- a stand-in network

#[derive(Default)]
struct Scripted {
    answers: RefCell<Vec<Reply>>,
    asked: RefCell<Vec<(String, String, Vec<(String, String)>)>>,
}

impl Scripted {
    fn with(r: Vec<Reply>) -> Scripted {
        Scripted { answers: RefCell::new(r), ..Default::default() }
    }
}

impl Net for Scripted {
    fn get(&self, host: &str, path: &str, headers: &[(&str, &str)]) -> Result<Reply, String> {
        self.asked.borrow_mut().push((host.into(), path.into(), headers.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()));
        let mut a = self.answers.borrow_mut();
        if a.is_empty() {
            return Err("no answer scripted".into());
        }
        Ok(a.remove(0))
    }
    fn post_form(&self, host: &str, path: &str, form: &str) -> Result<Reply, String> {
        self.get(host, path, &[("form", form)])
    }
    fn post_json(&self, host: &str, path: &str, headers: &[(&str, &str)], body: &str) -> Result<Reply, String> {
        let mut h = headers.to_vec();
        h.push(("json", body));
        self.get(host, path, &h)
    }
}

fn ok(body: &str) -> Reply {
    Reply { status: 200, body: body.into(), ..Default::default() }
}

#[test]
fn a_feed_is_asked_if_modified_since_and_a_304_changes_nothing() {
    let t = Target::YoutubeChannel { id: "UCBJycsmduvYEL83R_U4JriQ".into(), handle: String::new() };
    let net = Scripted::with(vec![Reply { status: 200, body: text("youtube_channel.xml"), last_modified: Some("Mon, 28 Sep 2026 10:00:00 GMT".into()), retry_after: None }, Reply { status: 304, ..Default::default() }]);
    let first = watchlist::read_one(&net, &t, None, None, false, NOW).unwrap();
    assert_eq!(first.items.len(), 3);
    let second = watchlist::read_one(&net, &t, first.last_modified.as_deref(), None, false, NOW).unwrap();
    assert!(second.unchanged);
    let asked = net.asked.borrow();
    assert!(asked[0].2.is_empty());
    assert_eq!(asked[1].2, vec![("If-Modified-Since".to_string(), "Mon, 28 Sep 2026 10:00:00 GMT".to_string())]);
    assert_eq!(asked[1].0, "www.youtube.com");
}

#[test]
fn a_site_that_says_slow_down_is_obeyed_and_failures_back_off_to_a_day() {
    let t = Target::Reddit { sub: "rust".into() };
    let net = Scripted::with(vec![Reply { status: 429, retry_after: Some(7200), ..Default::default() }]);
    let e = watchlist::read_one(&net, &t, None, None, false, NOW).unwrap_err();
    assert_eq!(e.retry_after, Some(7200));
    assert_eq!(net.asked.borrow()[0].1, "/r/rust/new/.rss");
    let mut w = Watch::default();
    w.add(vec![t.clone()], NOW).unwrap();
    let key = t.key();
    w.failed(&key, &e, NOW, 60);
    assert_eq!(w.list[0].next_due, NOW + 7200, "the server's two hours, over the doubled hour");
    for _ in 0..10 {
        w.failed(&key, &Failed { why: "x".into(), retry_after: None }, NOW, 60);
    }
    assert_eq!(w.list[0].next_due, NOW + 86_400, "never more than a day");
    w.took(&key, Fetched::default(), NOW, 60);
    assert_eq!((w.list[0].failures, w.list[0].next_due), (0, NOW + 3600));
    // Never more often than every 30 minutes, whatever the setting.
    w.took(&key, Fetched::default(), NOW, 5);
    assert_eq!(w.list[0].next_due, NOW + 1800);
}

#[test]
fn a_video_seen_twice_says_how_fast_it_moved() {
    let t = Target::YoutubeChannel { id: "UCx".into(), handle: String::new() };
    let mut w = Watch::default();
    w.add(vec![t.clone()], NOW).unwrap();
    let mut items = watchlist::parse_youtube_feed(&text("youtube_channel.xml")).unwrap();
    w.took(&t.key(), Fetched { items: items.clone(), ..Default::default() }, NOW, 60);
    items[0].views = Some(1_950_154);
    w.took(&t.key(), Fetched { items, ..Default::default() }, NOW + 7200, 60);
    let s = w.seen.iter().find(|s| s.id == "rayrrXot17M").unwrap();
    assert_eq!(s.views_before, Some((NOW, 1_900_154)));
    let d = analysis::watch_digest(&w, NOW + 7200);
    let moving = d.sections.iter().find(|(h, _)| h.starts_with("Moving fastest")).unwrap();
    assert!(moving.1[0].contains("+50,000 views in the last 2 hours"), "{:?}", moving.1);
}

#[test]
fn youtube_searches_are_budgeted_to_the_pacific_day_and_never_past_googles_hundred() {
    let mut q = Quota::default();
    let day = watchlist::pacific_day(NOW);
    for _ in 0..3 {
        assert!(q.take(day, 3));
    }
    assert!(!q.take(day, 3), "the fourth of three is refused");
    assert_eq!(q.left(day, 3), 0);
    assert!(q.take(day + 1, 3), "a new Pacific day starts again");
    let mut big = Quota::default();
    let taken = (0..500).filter(|_| big.take(day, 500)).count();
    assert_eq!(taken, 100, "Google's ceiling holds whatever the setting says");
    // Midnight UTC is still the day before in California.
    assert_eq!(watchlist::pacific_day(atlas::civil::days_from_civil(2026, 9, 28) as u64 * DAY), atlas::civil::days_from_civil(2026, 9, 27));
}

#[test]
fn a_search_needs_a_key_and_budget_and_then_costs_two_calls() {
    let t = Target::YoutubeSearch { query: "video editing".into() };
    let none = Scripted::default();
    assert!(watchlist::read_one(&none, &t, None, None, true, NOW).unwrap_err().why.contains("API key"));
    assert!(watchlist::read_one(&none, &t, None, Some("k"), false, NOW).unwrap_err().why.contains("budget"));
    assert!(none.asked.borrow().is_empty(), "nothing went out without a key or budget");
    let net = Scripted::with(vec![
        ok(r#"{"items":[{"id":{"kind":"youtube#video","videoId":"abc123def45"}}]}"#),
        ok(r#"{"items":[{"id":"abc123def45","snippet":{"title":"Edit faster","channelTitle":"Northwind","publishedAt":"2026-09-25T10:00:00Z"},"statistics":{"viewCount":"48210","likeCount":"1900","commentCount":"88"}}]}"#),
    ]);
    let got = watchlist::read_one(&net, &t, None, Some("k"), true, NOW).unwrap();
    assert_eq!(got.items[0].views, Some(48_210));
    assert_eq!(got.items[0].who, "Northwind");
    let asked = net.asked.borrow();
    assert_eq!(asked.len(), 2);
    assert!(asked[0].1.starts_with("/youtube/v3/search?") && asked[0].1.contains("order=viewCount") && asked[0].1.contains("publishedAfter=2026-09-21"));
    assert!(asked[1].1.starts_with("/youtube/v3/videos?"));
}

#[test]
fn a_channel_named_by_handle_is_looked_up_once_and_remembered() {
    let t = Target::YoutubeChannel { id: String::new(), handle: "@northwind".into() };
    let net = Scripted::with(vec![ok("<html><link rel=\"canonical\" href=\"https://www.youtube.com/channel/UCBJycsmduvYEL83R_U4JriQ\"></html>"), ok(&text("youtube_channel.xml"))]);
    let got = watchlist::read_one(&net, &t, None, None, false, NOW).unwrap();
    assert_eq!(got.resolved, Some(Target::YoutubeChannel { id: "UCBJycsmduvYEL83R_U4JriQ".into(), handle: "@northwind".into() }));
    let mut w = Watch::default();
    w.add(vec![t.clone()], NOW).unwrap();
    w.took(&t.key(), got, NOW, 60);
    assert_eq!(w.list[0].target.key(), "yt:UCBJycsmduvYEL83R_U4JriQ");
    assert!(w.seen.iter().all(|s| s.from == "yt:UCBJycsmduvYEL83R_U4JriQ"));
}

// ---------------------------------------------------------------- what you ask to watch

#[test]
fn what_you_name_becomes_the_right_source_and_tiktok_instagram_x_are_refused_with_why() {
    let cfg = SocialConfig::default();
    let one = |s: &str| watchlist::parse_target(s, &cfg);
    assert_eq!(one("watch #rustlang").unwrap(), vec![Target::MastodonTag { instance: "mastodon.social".into(), tag: "rustlang".into() }]);
    assert_eq!(one("watch r/VideoEditing").unwrap(), vec![Target::Reddit { sub: "videoediting".into() }]);
    assert_eq!(one("watch the youtube channel @Northwind").unwrap(), vec![Target::YoutubeChannel { id: String::new(), handle: "@Northwind".into() }]);
    assert_eq!(one("watch https://www.youtube.com/channel/UCBJycsmduvYEL83R_U4JriQ").unwrap(), vec![Target::YoutubeChannel { id: "UCBJycsmduvYEL83R_U4JriQ".into(), handle: String::new() }]);
    assert_eq!(one("watch maya.bsky.social on bluesky").unwrap(), vec![Target::BlueskyProfile { handle: "maya.bsky.social".into() }]);
    assert_eq!(one("watch hacker news").unwrap(), vec![Target::HackerNews { query: String::new() }]);
    assert_eq!(one("watch google trends").unwrap(), vec![Target::GoogleTrends { geo: "US".into() }]);
    assert_eq!(one("watch the topic color grading").unwrap(), vec![Target::HackerNews { query: "color grading".into() }, Target::YoutubeSearch { query: "color grading".into() }]);
    for s in ["watch @maya on tiktok", "watch maya on instagram", "watch @maya on twitter"] {
        let e = one(s).unwrap_err();
        assert!(e.contains("on a schedule") && e.contains("one page when you ask"), "{e}");
    }
    let mut w = Watch::default();
    assert_eq!(w.add(one("watch #rustlang").unwrap(), NOW).unwrap().len(), 1);
    assert!(w.add(one("watch #RustLang").unwrap(), NOW).unwrap().is_empty(), "the same source twice is one");
    assert_eq!(w.remove("rustlang"), vec!["Mastodon #rustlang (on mastodon.social)".to_string()]);
}

// ---------------------------------------------------------------- one page, when asked

#[test]
fn one_page_only_from_the_three_sites_over_https_and_spaced() {
    assert!(onepage::check_url("https://www.tiktok.com/@maya/video/7418").is_ok());
    assert!(onepage::check_url("https://x.com/maya/status/1").is_ok());
    assert!(onepage::check_url("https://www.instagram.com/p/abc/").is_ok());
    assert!(onepage::check_url("http://x.com/maya").is_err(), "never plain http");
    assert!(onepage::check_url("https://x.com.evil.example/maya").is_err());
    assert!(onepage::check_url("https://example.com").is_err());
    let mut s = onepage::Spacing::default();
    assert!(s.may(onepage::Site::X, NOW).is_ok());
    assert_eq!(s.may(onepage::Site::X, NOW + 30), Err(90));
    assert!(s.may(onepage::Site::Tiktok, NOW + 30).is_ok(), "per site");
    assert!(s.may(onepage::Site::X, NOW + 120).is_ok());
}

#[test]
fn a_pages_counts_are_read_as_the_site_shows_them() {
    let c = onepage::counts_on_page("Maya 12.4K Followers 310 Following 1.2M views 8,450 likes Comments 96 Share");
    let get = |w: &str| c.iter().find(|x| x.what == w).map(|x| x.value);
    assert_eq!(get("followers"), Some(12_400));
    assert_eq!(get("views"), Some(1_200_000));
    assert_eq!(get("likes"), Some(8_450));
    assert_eq!(get("comments"), Some(96));
    let said = onepage::said_about(onepage::Site::Tiktok, "https://www.tiktok.com/@maya", "Maya 1.2M views");
    assert!(said.contains("1.2M views") && said.contains("rounded"), "{said}");
    assert!(onepage::said_about(onepage::Site::X, "u", "  ").contains("nothing readable"));
}

// ---------------------------------------------------------------- the official APIs' answers

#[test]
fn the_youtube_data_api_is_three_calls_and_its_answer_is_read_whole() {
    let net = Scripted::with(vec![
        ok(r#"{"items":[{"id":"UCme","snippet":{"customUrl":"@jordanedits"},"statistics":{"viewCount":"50210","subscriberCount":"1180","hiddenSubscriberCount":false,"videoCount":"42"},"contentDetails":{"relatedPlaylists":{"uploads":"UUme"}}}]}"#),
        ok(r#"{"items":[{"contentDetails":{"videoId":"v1"}},{"contentDetails":{"videoId":"v2"}}]}"#),
        ok(r#"{"items":[{"id":"v1","snippet":{"title":"Stop doing this","publishedAt":"2026-09-26T15:00:00Z"},"contentDetails":{"duration":"PT1M3S"},"statistics":{"viewCount":"812","likeCount":"77","commentCount":"9"}},{"id":"v2","snippet":{"title":"Older"},"contentDetails":{"duration":"PT10M"},"statistics":{"viewCount":"4000"}}]}"#),
    ]);
    let recs = apis::youtube_own(&net, "key", "@jordanedits", NOW).unwrap();
    let a = accounts(&recs)[0];
    assert_eq!((a.followers, a.posts, a.m.views), (Some(1180), Some(42), Some(50210)));
    let p = posts(&recs);
    assert_eq!((p[0].m.views, p[0].m.likes, p[0].seconds), (Some(812), Some(77), Some(63.0)));
    assert_eq!((p[1].m.likes, p[1].m.comments), (None, None), "hidden counts stay unknown");
    assert!(net.asked.borrow()[0].1.contains("forHandle=%40jordanedits"));
    assert_eq!(apis::iso_duration("P1DT2H"), Some(93_600.0));
    assert_eq!(apis::iso_duration("nonsense"), None);
}

#[test]
fn analytics_rows_are_read_by_their_headers_not_their_order() {
    let v: serde_json::Value = serde_json::from_str(
        r#"{"columnHeaders":[{"name":"averageViewPercentage"},{"name":"video"},{"name":"views"},{"name":"averageViewDuration"}],"rows":[[61.5,"v1",812,38.0]]}"#,
    )
    .unwrap();
    let rows = apis::analytics_rows(&v);
    assert_eq!(rows[0].0, "v1");
    assert_eq!((rows[0].1.avg_view_pct, rows[0].1.views, rows[0].1.avg_view_secs), (Some(61.5), Some(812), Some(38.0)));
}

#[test]
fn the_seven_day_google_sign_in_is_named_before_and_after_it_lapses() {
    let g = apis::GoogleSignIn { client_id: "c".into(), client_secret: "s".into(), refresh_token: "r".into(), obtained: NOW };
    assert!(g.lapsing(NOW + DAY, true).is_none());
    assert!(g.lapsing(NOW + 6 * DAY + 1, true).unwrap().contains("within a day"));
    assert!(g.lapsing(NOW + 7 * DAY, true).unwrap().contains("has lapsed"));
    assert!(g.lapsing(NOW + 30 * DAY, false).is_none(), "a published app's sign-in doesn't lapse this way");
    let net = Scripted::with(vec![Reply { status: 400, body: r#"{"error":"invalid_grant","error_description":"Token has been expired or revoked."}"#.into(), ..Default::default() }]);
    let e = apis::google_access(&net, &g).unwrap_err();
    assert!(e.contains("seven days") && e.contains("Social page"), "{e}");
    let form = &net.asked.borrow()[0].2[0].1;
    assert!(form.contains("grant_type=refresh_token") && form.contains("refresh_token=r"));
}

#[test]
fn the_google_sign_in_uses_pkce_and_checks_its_state() {
    // Worked out separately with Python's hashlib and base64.urlsafe_b64encode
    // (29 Sep 2026): base64url of SHA-256, no padding, no '+' or '/'.
    assert_eq!(apis::pkce_challenge("dBjftJeZ4CVP-mJ92K27uhbUJU1p1r_wW1gFWFOEjXk"), "ngF5GsXcbwljx6u133FFr3Xht9xooA_DuaX_3QwODtc");
    let url = apis::google_consent_url("id.apps.googleusercontent.com", "http://127.0.0.1:5555", "st", "ch");
    assert!(url.contains("code_challenge_method=S256") && url.contains("yt-analytics.readonly") && url.contains("access_type=offline"));
    assert_eq!(apis::code_from_redirect("GET /?state=st&code=4%2F0Ab HTTP/1.1", "st").unwrap(), "4/0Ab");
    assert!(apis::code_from_redirect("GET /?state=other&code=x HTTP/1.1", "st").unwrap_err().contains("didn't match"));
    assert!(apis::code_from_redirect("GET /?error=access_denied&state=st HTTP/1.1", "st").unwrap_err().contains("access_denied"));
}

#[test]
fn instagram_insights_are_read_by_name() {
    let v: serde_json::Value = serde_json::from_str(
        r#"{"data":[{"name":"views","period":"lifetime","values":[{"value":5310}]},{"name":"reach","values":[{"value":4002}]},{"name":"saved","values":[{"value":57}]},{"name":"ig_reels_avg_watch_time","values":[{"value":8450}]}]}"#,
    )
    .unwrap();
    let m = apis::instagram_insights(&v);
    assert_eq!((m.views, m.reach, m.saves, m.avg_view_secs), (Some(5310), Some(4002), Some(57), Some(8.45)));
    assert_eq!(m.likes, None);
}

// ---------------------------------------------------------------- through the daemon

mod through_the_daemon {
    use super::*;
    use atlas::config::Config;
    use atlas::daemon::Daemon;
    use atlas::hub::Page;
    use atlas::intent::Intent;
    use atlas::platform::mock::MockPlatform;
    use atlas::platform::Monitor;
    use atlas::proactive::{Proactive, ProactiveConfig};
    use atlas::store::Store;

    fn plat() -> MockPlatform {
        MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
    }

    fn cfg() -> &'static Config {
        Box::leak(Box::new(Config::load(Path::new("config")).unwrap()))
    }

    #[test]
    fn the_schedules_ship_off_and_asking_works_without_them() {
        let c = cfg();
        let s = &c.tools.as_ref().unwrap().workday.social;
        assert!(s.enabled && !s.own_refresh && !s.scan, "nothing fetches on a schedule until turned on");
        assert_eq!(*s, SocialConfig::default(), "tools.yaml and the code agree");
        let reg = atlas::settings::registry(c.tools.as_ref().unwrap());
        for key in ["workday.social.own_refresh", "workday.social.scan"] {
            let it = reg.items.iter().find(|i| i.key == key).unwrap_or_else(|| panic!("{key} is in Settings"));
            assert_eq!(it.value, atlas::settings::Value::Toggle(false));
            assert!(it.weight.needs_confirming(), "{key} reaches outside the machine");
        }
    }

    #[test]
    fn importing_then_asking_answers_from_the_data() {
        let p = plat();
        let dir = tmp("daemon");
        let mut d = Daemon::new(cfg(), &p, None, Store::new(dir.clone()), Proactive::new(ProactiveConfig::default()));
        let before = d.execute(&Intent::Social("how did my last video do".into()));
        assert!(before.contains("no videos"), "{before}");
        let path = std::fs::canonicalize(fx("exports/youtube_studio")).unwrap();
        let said = d.execute(&Intent::Social(format!("import {}", path.display())));
        assert!(said.starts_with("Read your YouTube Studio export: 3 posts"), "{said}");
        assert!(said.contains("subscriber total"), "what the file lacks is said: {said}");
        let again = d.execute(&Intent::Social(format!("import {}", path.display())));
        assert!(again.contains("already on record, unchanged"), "{again}");
        let last = d.execute(&Intent::Social("how did my last video do".into()));
        assert!(last.contains("\"Why your exports look soft\"") && last.contains("2,010 views") && last.contains("62% watched"), "{last}");
        let tiktok = std::fs::canonicalize(fx("exports/tiktok")).unwrap();
        d.execute(&Intent::Social(format!("import \"{}\"", tiktok.display())));
        let f = d.execute(&Intent::Social("how many followers do i have".into()));
        assert!(f.contains("TikTok: 3"), "{f}");
        let x = d.execute(&Intent::Social("watch @maya on tiktok".into()));
        assert!(x.contains("on a schedule"), "{x}");
        let walls = d.execute(&Intent::Social("what can't you get".into()));
        assert!(walls.contains("Premium") && walls.contains("retention"), "{walls}");
        let html = atlas::hublive::reply(&mut d, atlas::server::Action::Hub(Page::Social)).body;
        assert!(html.contains("<main id=main"), "the page renders");
        assert!(html.contains("YouTube") && html.contains("TikTok"), "both platforms are listed");
        assert!(html.contains("What can&#39;t be had") || html.contains("What can't be had"), "the walls are on the page");
        // 29 Sep 2026: Threads and Facebook Pages joined the platforms read, so
    // they are named among the missing too.
    assert!(html.contains("Nothing from Instagram, X, LinkedIn, Bluesky, Threads, Facebook yet"), "the missing ones are named");
    assert!(html.contains("How to set each one up") && html.contains("Publish app") && html.contains("Sandbox"), "each route's setup is spelled out");
    assert!(html.contains("name=tiktok-finish") || html.contains("value='tiktok-finish'"), "TikTok's paste-back step is on the page");
        // The page's own buttons: add a source, and a key with the vault locked.
        let post = |d: &mut Daemon, fields: &[(&str, &str)]| {
            let r = atlas::hublive::reply(d, atlas::server::Action::HubPost { path: "/hub/social".into(), fields: fields.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect() });
            assert_eq!(r.status, 303, "back to the page, saying what happened");
            atlas::hub::urldecode(&r.body)
        };
        let back = post(&mut d, &[("what", "watch"), ("target", "#rustlang")]);
        assert!(back.contains("Watching Mastodon #rustlang"), "{back}");
        assert!(back.contains("Scanning on a schedule is off"), "{back}");
        let back = post(&mut d, &[("what", "key"), ("name", "youtube"), ("secret", "AIza-not-real")]);
        assert!(back.contains("locked"), "{back}");
        let html = atlas::hublive::reply(&mut d, atlas::server::Action::Hub(Page::Social)).body;
        assert!(html.contains("Mastodon #rustlang (on mastodon.social)") && html.contains("not read yet"), "the watched source is listed");
        assert!(!html.contains("AIza-not-real"), "a key is never shown back");
        // 5 Oct 2026: YouTube Analytics signs in with Atlas's own Google
        // registration; a copy built without its key says so.
        // The sites you sign in to yourself, in Atlas's own browser window.
        let html = atlas::hublive::reply(&mut d, atlas::server::Action::Hub(Page::Social)).body;
        for (name, _) in atlas::social::SIGN_IN_SITES {
            assert!(html.contains(&format!("Sign in to {name}")), "{name} has no sign-in button");
        }
        let back = post(&mut d, &[("what", "browser-signin"), ("site", "evil.example")]);
        assert!(back.contains("isn't one of the sites listed"), "{back}");
        let back = post(&mut d, &[("what", "google")]);
        if atlas::oauthlink::google_secret().is_none() {
            assert!(back.contains("built without Google's sign-in key"), "{back}");
        }
        // Carried only on a private line, like a passphrase.
        assert!(atlas::server::Action::HubPost { path: "/hub/social".into(), fields: vec![("secret".into(), "x".into())] }.carries_a_secret());
        let _ = std::fs::remove_dir_all(dir);
    }
}

// ---------------------------------------------------------------- Threads, Facebook Pages, TikTok (29 Sep 2026)
//
// Hand-built answers, NOT saved from the live services (these need Eric's
// own tokens): each copies the shape in the platform's reference as read on
// 29 Sep 2026 -- Threads Insights (`/{media}/insights` with `values`, the
// account's `followers_count` as `total_value`), the Graph API's Page posts
// with `reactions.summary` / `comments.summary` and `post_media_view`, and
// TikTok's v2 `user/info` and `video/list` (`data.videos`, `error.code`).

#[test]
fn threads_reads_the_account_its_followers_and_each_posts_insights() {
    let net = Scripted::with(vec![
        ok(r#"{"id":"1789","username":"jordan.makes"}"#),
        ok(r#"{"data":[{"name":"followers_count","period":"day","title":"followers_count","total_value":{"value":431},"id":"1789/insights/followers_count/day"}]}"#),
        ok(r#"{"data":[{"id":"p1","media_type":"TEXT_POST","text":"Three edits I stopped making","timestamp":"2026-09-27T14:05:00+0000","permalink":"https://www.threads.com/@jordan.makes/post/p1"},{"id":"p2","media_type":"REPOST_FACADE","timestamp":"2026-09-26T10:00:00+0000"}],"paging":{}}"#),
        ok(r#"{"data":[{"name":"views","period":"lifetime","values":[{"value":2210}]},{"name":"likes","period":"lifetime","values":[{"value":88}]},{"name":"replies","period":"lifetime","values":[{"value":12}]},{"name":"reposts","period":"lifetime","values":[{"value":5}]},{"name":"quotes","period":"lifetime","values":[{"value":1}]},{"name":"shares","period":"lifetime","values":[{"value":3}]}]}"#),
    ]);
    let recs = apis::threads_own(&net, "tok", NOW).unwrap();
    let a = accounts(&recs)[0];
    assert_eq!((a.platform, a.handle.as_str(), a.followers), (Platform::Threads, "jordan.makes", Some(431)));
    let p = posts(&recs);
    assert_eq!(p.len(), 1, "a repost of someone else's is not yours to measure");
    assert_eq!((p[0].m.views, p[0].m.likes, p[0].m.comments, p[0].m.reposts, p[0].m.quotes, p[0].m.shares), (Some(2210), Some(88), Some(12), Some(5), Some(1), Some(3)));
    assert_eq!(p[0].posted, apis::rfc3339("2026-09-27T14:05:00Z"), "Meta's +0000 offset is read");
    let asked = net.asked.borrow();
    assert!(asked.iter().all(|(h, _, _)| h == "graph.threads.net"));
    assert!(asked[1].1.starts_with("/v1.0/1789/threads_insights?metric=followers_count"));
    assert_eq!(asked.len(), 4, "no insights asked for the repost");
}

#[test]
fn a_facebook_page_reads_views_reactions_and_comments_and_keeps_absent_shares_unknown() {
    let net = Scripted::with(vec![
        ok(r#"{"id":"1001","name":"Northwind Studio","followers_count":2045}"#),
        ok(r#"{"data":[{"id":"1001_1","message":"New tutorial is up","created_time":"2026-09-25T17:30:00+0000","permalink_url":"https://www.facebook.com/1001/posts/1","shares":{"count":7},"reactions":{"data":[],"summary":{"total_count":64}},"comments":{"data":[],"summary":{"total_count":9}}},{"id":"1001_2","created_time":"2026-09-20T09:00:00+0000","reactions":{"data":[],"summary":{"total_count":3}},"comments":{"data":[],"summary":{"total_count":0}}}]}"#),
        ok(r#"{"data":[{"name":"post_media_view","period":"lifetime","values":[{"value":3120}]}]}"#),
        Reply { status: 400, body: r#"{"error":{"message":"(#100) The value must be a valid insights metric","code":100}}"#.into(), ..Default::default() },
    ]);
    let recs = apis::facebook_page_own(&net, "pagetok", NOW).unwrap();
    let a = accounts(&recs)[0];
    assert_eq!((a.platform, a.followers), (Platform::Facebook, Some(2045)));
    let p = posts(&recs);
    assert_eq!((p[0].m.views, p[0].m.likes, p[0].m.comments, p[0].m.shares), (Some(3120), Some(64), Some(9), Some(7)));
    assert_eq!((p[1].m.views, p[1].m.shares), (None, None), "a refused insight and an absent share count stay unknown, never zero");
    let asked = net.asked.borrow();
    assert!(asked[0].1.starts_with(&format!("/{}/me?", apis::GRAPH_VERSION)));
    assert!(asked[2].1.contains("metric=post_media_view"), "the metric that replaced post_impressions");
}

#[test]
fn tiktok_refreshes_its_token_reads_videos_and_says_its_own_refusals() {
    let s = apis::TikTokSignIn { client_key: "ck".into(), client_secret: "cs".into(), redirect: "https://example.com/tt".into(), refresh_token: "r1".into(), state: String::new(), obtained: NOW - DAY };
    let net = Scripted::with(vec![
        ok(r#"{"access_token":"act.1","expires_in":86400,"open_id":"o1","refresh_expires_in":31536000,"refresh_token":"rft.2","scope":"user.info.basic,video.list","token_type":"Bearer"}"#),
        ok(r#"{"data":{"user":{"open_id":"o1","display_name":"Jordan","follower_count":3104,"following_count":120,"likes_count":45210,"video_count":61}},"error":{"code":"ok","message":"","log_id":"x"}}"#),
        ok(r#"{"data":{"videos":[{"id":"7301","title":"","video_description":"The cut nobody notices #editing","create_time":1790420000,"duration":23,"share_url":"https://www.tiktok.com/@jordan/video/7301","view_count":18400,"like_count":1320,"comment_count":41,"share_count":77}],"cursor":1790420000000,"has_more":false},"error":{"code":"ok","message":"","log_id":"y"}}"#),
    ]);
    let (access, refresh) = apis::tiktok_access(&net, &s).unwrap();
    assert_eq!((access.as_str(), refresh.as_str()), ("act.1", "rft.2"), "the new refresh token replaces the old");
    let recs = apis::tiktok_own(&net, &access, NOW).unwrap();
    let a = accounts(&recs)[0];
    assert_eq!((a.followers, a.posts, a.m.likes), (Some(3104), Some(61), Some(45210)));
    let p = posts(&recs);
    assert_eq!(p[0].text, "The cut nobody notices #editing", "the description stands in for an empty title");
    assert_eq!((p[0].m.views, p[0].m.shares, p[0].seconds, p[0].posted), (Some(18400), Some(77), Some(23.0), Some(1_790_420_000)));
    assert_eq!((p[0].m.avg_view_pct, p[0].m.avg_view_secs), (None, None), "TikTok gives no retention and none is made");
    let asked = net.asked.borrow();
    assert!(asked[0].2[0].1.contains("grant_type=refresh_token") && asked[0].2[0].1.contains("refresh_token=r1"));
    assert!(asked[1].2.iter().any(|(k, v)| k == "Authorization" && v == "Bearer act.1"));
    assert!(asked[2].1.starts_with("/v2/video/list/?fields=") && asked[2].2.iter().any(|(k, v)| k == "json" && v.contains("max_count")));
    drop(asked);
    let refused = Scripted::with(vec![ok(r#"{"data":{},"error":{"code":"scope_not_authorized","message":"The user did not authorize the scope required for completing this request.","log_id":"z"}}"#)]);
    assert!(apis::tiktok_own(&refused, "a", NOW).unwrap_err().contains("did not authorize the scope"));
    let lapsed = Scripted::with(vec![Reply { status: 400, body: r#"{"error":"invalid_grant","error_description":"Refresh token is invalid or expired."}"#.into(), ..Default::default() }]);
    assert!(apis::tiktok_access(&lapsed, &s).unwrap_err().contains("Sign in again"));
}

#[test]
fn tiktoks_sign_in_is_a_consent_page_and_a_pasted_address_with_its_state() {
    let s = apis::TikTokSignIn { client_key: "ck".into(), client_secret: "cs".into(), redirect: "https://example.com/tt".into(), refresh_token: String::new(), state: "st9".into(), obtained: 0 };
    let url = apis::tiktok_consent_url(&s);
    assert!(url.starts_with("https://www.tiktok.com/v2/auth/authorize/?client_key=ck"));
    assert!(url.contains("video.list") && url.contains("user.info.stats") && url.contains("state=st9") && url.contains("redirect_uri=https%3A%2F%2Fexample.com%2Ftt"));
    assert_eq!(apis::code_from_address("https://example.com/tt?code=abc%2A1&scopes=user.info.basic&state=st9", "st9").unwrap(), "abc*1");
    assert!(apis::code_from_address("https://example.com/tt?code=abc&state=other", "st9").is_err());
    let net = Scripted::with(vec![ok(r#"{"access_token":"a","refresh_token":"r","expires_in":86400}"#)]);
    assert_eq!(apis::tiktok_exchange(&net, &s, "abc*1").unwrap().1, "r");
    let form = &net.asked.borrow()[0].2[0].1;
    assert!(form.contains("grant_type=authorization_code") && form.contains("code=abc%2A1") && form.contains("client_secret=cs"));
}

#[test]
fn watching_is_heard_by_its_marks_and_unwatching_takes_only_what_was_named() {
    let c = atlas::config::Config::load(Path::new("config")).unwrap();
    let parser = atlas::intent::Parser::new(&c.commands);
    use atlas::intent::Intent;
    for said in ["watch @mkbhd on YouTube", "follow the #rustlang hashtag on Mastodon", "stop watching @mkbhd on youtube", "watch r/editors", "keep an eye on #shorts"] {
        assert!(matches!(parser.parse(said), Intent::Social(_)), "{said} -> {:?}", parser.parse(said));
    }
    assert!(!matches!(parser.parse("watch my hands"), Intent::Social(_)));
    assert!(matches!(parser.parse("follow theverge.com"), Intent::Feeds(_)), "a site's feed is still feeds");
    assert!(!watchlist::spoken_watch("watch the disk and tell me when it's full"));
    let cfg = SocialConfig::default();
    assert_eq!(watchlist::parse_target("follow the #rustlang hashtag on Mastodon", &cfg).unwrap(), vec![Target::MastodonTag { instance: "mastodon.social".into(), tag: "rustlang".into() }]);
    assert_eq!(watchlist::parse_target("watch @mkbhd on YouTube", &cfg).unwrap(), vec![Target::YoutubeChannel { id: String::new(), handle: "@mkbhd".into() }]);
    assert!(watchlist::parse_target("watch @zuck on threads", &cfg).unwrap_err().contains("Meta's approval"));
    let mut w = Watch::default();
    w.add(vec![Target::YoutubeChannel { id: String::new(), handle: "@x".into() }, Target::MastodonTag { instance: "mastodon.social".into(), tag: "xbox".into() }, Target::Reddit { sub: "editors".into() }], NOW).unwrap();
    assert_eq!(w.remove("@x on youtube"), vec!["YouTube: @x".to_string()], "@x is not every source with an x in it");
    assert_eq!(w.remove("the #xbox hashtag on Mastodon").len(), 1);
    assert_eq!(w.list.len(), 1);
}

#[test]
fn the_brief_uses_a_fresh_checked_summary_and_otherwise_the_figures() {
    let mut w = Watch::default();
    w.add(vec![Target::HackerNews { query: String::new() }], NOW).unwrap();
    let items = watchlist::parse_hn(&text("hn_front_page.json")).unwrap();
    let at = items[0].at.unwrap();
    w.took("hn:", Fetched { items, ..Default::default() }, at + 3600, 60);
    let plain = analysis::brief_digest(&w, at + 3600).unwrap();
    assert!(plain.starts_with("On Hacker News: "), "{plain}");
    w.summary = Some((at, "Tooling posts led the front page.".into()));
    assert_eq!(analysis::brief_digest(&w, at + 3600).unwrap(), "Tooling posts led the front page.");
    assert!(analysis::brief_digest(&w, at + 40 * 3600).map_or(true, |s| s != "Tooling posts led the front page."), "a day-and-a-half-old summary isn't used");
}

// ---------------------------------------------------------------- the smaller pieces, called directly
//
// Each of these was reached only from inside `social` (29 Sep 2026); a direct
// test pins what it says rather than leaving it to whichever answer uses it.

#[test]
fn counts_days_and_platform_names_read_as_a_person_says_them() {
    assert_eq!(atlas::social::count_said(999), "999");
    assert_eq!(atlas::social::count_said(1_234), "1,234");
    assert_eq!(atlas::social::count_said(1_260_000), "1.3M");
    assert_eq!(atlas::social::count_said(12_000_000), "12M");
    assert_eq!(atlas::social::day_label(atlas::social::social_day(NOW)), "28 Sep");
    assert_eq!(Platform::in_words("how's my tiktok doing"), Some(Platform::Tiktok));
    assert_eq!(Platform::in_words("my last reel"), Some(Platform::Instagram));
    assert_eq!(Platform::in_words("followers on threads"), Some(Platform::Threads));
    assert_eq!(Platform::in_words("my facebook page"), Some(Platform::Facebook));
    assert_eq!(Platform::in_words("how did my last video do"), None, "no platform named is none, not a guess");
    assert!(Platform::Youtube.is_video() && Platform::Tiktok.is_video() && !Platform::Linkedin.is_video() && !Platform::Threads.is_video());
}

#[test]
fn the_book_lists_its_platforms_and_an_accounts_days_in_order() {
    let dir = tmp("platforms");
    let path = dir.join("s.jsonl");
    let acct = |pf: Platform, day: i64, f: u64| Record::Account(AccountSnap { platform: pf, handle: "me".into(), day, taken: NOW, followers: Some(f), following: None, posts: None, source: "t".into(), m: Metrics::default() });
    let today = atlas::social::social_day(NOW);
    let mut b = Book::load(&path);
    b.add(&path, vec![acct(Platform::Threads, today, 50), acct(Platform::Threads, today - 3, 40), acct(Platform::Facebook, today, 9)]).unwrap();
    assert_eq!(b.platforms(), vec![Platform::Threads, Platform::Facebook]);
    let days: Vec<i64> = b.account_days(Platform::Threads).iter().map(|a| a.day).collect();
    assert_eq!(days, vec![today - 3, today], "oldest first, whatever order they were written");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_address_is_found_in_what_you_said() {
    assert_eq!(onepage::url_in("read this tiktok https://www.tiktok.com/@maya/video/1. thanks").as_deref(), Some("https://www.tiktok.com/@maya/video/1"));
    assert_eq!(onepage::url_in("read this tiktok"), None);
}

#[test]
fn the_list_answers_read_whole_youtube_tiktok_and_threads() {
    let yt: serde_json::Value = serde_json::from_str(r#"{"items":[{"id":"v9","snippet":{"title":"A","publishedAt":"2026-09-20T10:00:00Z"},"contentDetails":{"duration":"PT45S"},"statistics":{"viewCount":"10","likeCount":"2"}}]}"#).unwrap();
    let r = apis::youtube_videos(&yt, NOW, "the YouTube Data API");
    let p = posts(&r);
    assert_eq!((p[0].id.as_str(), p[0].seconds, p[0].m.views, p[0].m.comments), ("v9", Some(45.0), Some(10), None));
    let tt: serde_json::Value = serde_json::from_str(r#"{"data":{"videos":[{"id":"1","title":"t","create_time":1790000000,"view_count":5}]},"error":{"code":"ok"}}"#).unwrap();
    let r = apis::tiktok_videos(&tt, NOW);
    assert_eq!((posts(&r)[0].m.views, posts(&r)[0].m.likes), (Some(5), None), "a count TikTok didn't send stays unknown");
    let th: serde_json::Value = serde_json::from_str(r#"{"data":[{"name":"views","values":[{"value":7}]},{"name":"replies","values":[{"value":2}]}]}"#).unwrap();
    let m = apis::threads_insights(&th);
    assert_eq!((m.views, m.comments, m.likes), (Some(7), Some(2), None));
}

#[test]
fn instagram_reads_the_account_posts_and_insights_and_a_failing_insight_costs_only_that_post() {
    let net = Scripted::with(vec![
        ok(r#"{"user_id":"17","username":"jordan.makes","followers_count":2200,"follows_count":180,"media_count":94,"id":"17"}"#),
        ok(r#"{"data":[{"id":"m1","caption":"How I cut a reel","media_type":"VIDEO","media_product_type":"REELS","timestamp":"2026-09-26T18:00:00+0000","permalink":"https://www.instagram.com/reel/m1/","like_count":310,"comments_count":22},{"id":"m2","caption":"Old post","media_type":"IMAGE","media_product_type":"FEED","timestamp":"2026-09-01T09:00:00+0000","like_count":40,"comments_count":1}]}"#),
        ok(r#"{"data":[{"name":"views","values":[{"value":9100}]},{"name":"reach","values":[{"value":7000}]},{"name":"saved","values":[{"value":88}]},{"name":"shares","values":[{"value":41}]},{"name":"ig_reels_avg_watch_time","values":[{"value":6200}]}]}"#),
        Reply { status: 400, body: r#"{"error":{"message":"Unsupported request"}}"#.into(), ..Default::default() },
    ]);
    let recs = apis::instagram_own(&net, "tok", NOW).unwrap();
    assert_eq!(accounts(&recs)[0].followers, Some(2200));
    let p = posts(&recs);
    assert_eq!((p[0].m.views, p[0].m.likes, p[0].m.saves, p[0].m.avg_view_secs), (Some(9100), Some(310), Some(88), Some(6.2)));
    assert_eq!((p[1].m.likes, p[1].m.views), (Some(40), None));
    let asked = net.asked.borrow();
    assert!(asked[2].1.contains("ig_reels_avg_watch_time"), "a reel is asked for its watch time");
    assert!(!asked[3].1.contains("ig_reels_avg_watch_time"), "a feed post isn't");
    assert!(asked.iter().all(|(_, p, _)| !p.starts_with('/') || p.starts_with(&format!("/{}/", apis::GRAPH_VERSION))));
}

#[test]
fn meta_tokens_are_renewed_and_google_codes_are_exchanged() {
    let net = Scripted::with(vec![ok(r#"{"access_token":"IGnew","token_type":"bearer","expires_in":5183944}"#), ok(r#"{"access_token":"THnew","token_type":"bearer","expires_in":5183944}"#)]);
    assert_eq!(apis::instagram_refresh(&net, "IGold").unwrap(), "IGnew");
    assert_eq!(apis::threads_refresh(&net, "THold").unwrap(), "THnew");
    let asked = net.asked.borrow();
    assert!(asked[0].1.contains("grant_type=ig_refresh_token") && asked[0].1.contains("access_token=IGold"));
    assert!(asked[1].0 == "graph.threads.net" && asked[1].1.contains("grant_type=th_refresh_token"));
    drop(asked);
    let g = apis::GoogleSignIn { client_id: "cid".into(), client_secret: "cs".into(), refresh_token: String::new(), obtained: 0 };
    let net = Scripted::with(vec![ok(r#"{"access_token":"ya29","expires_in":3599,"refresh_token":"1//r","scope":"x","token_type":"Bearer"}"#)]);
    assert_eq!(apis::google_exchange(&net, &g, "4/0Ab", "http://127.0.0.1:5555", "ver").unwrap(), "1//r");
    let form = &net.asked.borrow()[0].2[0].1;
    assert!(form.contains("code_verifier=ver") && form.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A5555"));
    let none = Scripted::with(vec![ok(r#"{"access_token":"ya29","expires_in":3599}"#)]);
    assert!(apis::google_exchange(&none, &g, "c", "r", "v").unwrap_err().contains("no refresh token"));
}

#[test]
fn youtube_analytics_asks_for_retention_over_the_days_given() {
    let net = Scripted::with(vec![ok(r#"{"kind":"youtubeAnalytics#resultTable","columnHeaders":[{"name":"video"},{"name":"views"},{"name":"estimatedMinutesWatched"},{"name":"averageViewDuration"},{"name":"averageViewPercentage"}],"rows":[["v1",812,410.5,30,58.2]]}"#)]);
    let today = atlas::social::social_day(NOW);
    let rows = apis::youtube_analytics(&net, "ya29", today - 28, today - 1).unwrap();
    assert_eq!(rows[0].0, "v1");
    assert_eq!((rows[0].1.avg_view_pct, rows[0].1.watch_minutes), (Some(58.2), Some(410.5)));
    let asked = net.asked.borrow();
    assert_eq!(asked[0].0, "youtubeanalytics.googleapis.com");
    assert!(asked[0].1.contains("ids=channel%3D%3DMINE") && asked[0].1.contains("averageViewPercentage") && asked[0].1.contains("dimensions=video"));
    assert!(asked[0].2.iter().any(|(k, v)| k == "Authorization" && v == "Bearer ya29"));
}

#[test]
fn a_post_is_judged_on_views_then_impressions_then_interactions() {
    let mut p = PostSnap { platform: Platform::Linkedin, id: "l1".into(), day: 0, taken: 0, posted: None, text: String::new(), url: String::new(), seconds: None, source: "t".into(), m: Metrics::default() };
    assert_eq!(analysis::headline_figure(&p), None, "no figures, no judgement");
    p.m.likes = Some(4);
    p.m.comments = Some(1);
    assert_eq!(analysis::headline_figure(&p), Some(("interactions", 5)));
    p.m.impressions = Some(900);
    assert_eq!(analysis::headline_figure(&p), Some(("impressions", 900)));
    p.m.views = Some(300);
    assert_eq!(analysis::headline_figure(&p), Some(("views", 300)));
}

#[test]
fn the_overview_says_what_is_kept_and_names_what_is_not() {
    assert!(analysis::accounts_overview(&Book::default()).starts_with("Nothing on your accounts yet"));
    let today = atlas::social::social_day(NOW);
    let b = book_of(vec![Record::Account(AccountSnap { platform: Platform::Bluesky, handle: "me".into(), day: today, taken: NOW, followers: Some(40), following: None, posts: None, source: "t".into(), m: Metrics::default() })]);
    let s = analysis::accounts_overview(&b);
    assert!(s.starts_with("Bluesky: 0 posts and 1 day of account figures, latest 28 Sep"), "{s}");
    assert!(s.contains("Nothing from YouTube, TikTok, Instagram, X, LinkedIn, Threads and Facebook yet"), "{s}");
}

#[test]
fn the_social_page_escapes_what_it_shows_and_says_when_the_vault_is_locked() {
    use atlas::social::page::{render_social, Accounts, PlatformRow, View};
    let v = View {
        rows: vec![PlatformRow { name: "YouTube".into(), posts: 2, followers: String::new(), latest: "28 Sep".into(), source: "your <script> export".into() }],
        watching: vec![("r/<b>".into(), "not read yet".into())],
        keys: vec![("YouTube API key".into(), None), ("Threads token".into(), Some(true))],
        accounts: Accounts { youtube_channel: "@jordan'x".into(), tiktok: true, ..Default::default() },
        searches_left: 20,
        ..Default::default()
    };
    let html = render_social(&v);
    assert!(!html.contains("<script> export") && html.contains("your &lt;script&gt; export"), "what came from a file is escaped");
    assert!(!html.contains("r/<b>"));
    assert!(html.contains("not in the data"), "no follower figure is said, not shown as 0");
    assert!(html.contains("vault locked") && html.contains("kept"));
    assert!(!html.contains("value='@jordan'x'"), "a handle can't break out of its field");
    assert!(html.contains("name=tiktok value=on checked"), "what's on shows as on");
    assert!(html.contains("YouTube searches left today: 20"));
}

/// The real network, through Atlas's own TLS reader and parsers -- run by
/// hand (`--ignored`), never in the suite: it depends on the sites being up
/// and on the network it runs from. What it saw on 29 Sep 2026 is in the
/// round's report.
#[test]
#[ignore]
fn live_public_sources_answer_through_atlass_own_reader() {
    let net = apis::Https;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let targets = vec![
        Target::HackerNews { query: String::new() },
        Target::HackerNews { query: "rust".into() },
        Target::YoutubeChannel { id: "UCBJycsmduvYEL83R_U4JriQ".into(), handle: String::new() },
        Target::BlueskyProfile { handle: "bsky.app".into() },
        Target::MastodonTag { instance: "fosstodon.org".into(), tag: "rustlang".into() },
        Target::MastodonTag { instance: "mastodon.social".into(), tag: "rustlang".into() },
        Target::ProductHunt,
        Target::GoogleTrends { geo: "US".into() },
        Target::Reddit { sub: "rust".into() },
    ];
    let mut worked = 0;
    for t in &targets {
        match watchlist::read_one(&net, t, None, None, false, now) {
            Ok(f) => {
                worked += 1;
                let with_counts = f.items.iter().filter(|s| s.views.is_some() || s.likes.is_some() || s.points.is_some() || s.traffic.is_some()).count();
                eprintln!("LIVE ok   {}: {} items, {} with counts", t.label(), f.items.len(), with_counts);
            }
            Err(e) => eprintln!("LIVE fail {}: {} (retry after {:?})", t.label(), e.why, e.retry_after),
        }
        std::thread::sleep(std::time::Duration::from_millis(1500));
    }
    match apis::bluesky_own(&net, "bsky.app", now) {
        Ok(r) => eprintln!("LIVE ok   Bluesky own-account reader: {} records, followers {:?}", r.len(), accounts(&r).first().and_then(|a| a.followers)),
        Err(e) => eprintln!("LIVE fail Bluesky own-account reader: {e}"),
    }
    assert!(worked > 0, "nothing answered at all -- no network here?");
}

#[test]
fn the_sign_in_window_is_the_same_browser_without_headless() {
    let launch: Vec<String> = ["--headless=new", "--disable-gpu", "--remote-debugging-port=9222", "--user-data-dir=data/chrome-profile", "--no-first-run"]
        .iter().map(|s| s.to_string()).collect();
    let a = atlas::browser::sign_in_window_args(&launch, "https://www.instagram.com/accounts/login/");
    assert!(!a.iter().any(|x| x.starts_with("--headless")), "{a:?}");
    assert!(a.contains(&"--user-data-dir=data/chrome-profile".to_string()), "the same profile, so the sign-in is kept: {a:?}");
    assert_eq!(a.last().map(String::as_str), Some("https://www.instagram.com/accounts/login/"));
}
