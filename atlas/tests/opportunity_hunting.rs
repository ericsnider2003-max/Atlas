//! Opportunity hunting (29 Sep 2026): gigs, jobs, grants, contracts and
//! niches, read on a polite schedule, deduped, filtered by what you said you
//! want, weighed on `opportunity.rs`'s five axes, and brought as a short
//! list you can ask more about, drop, or save.
//!
//! The samples in `tests/fixtures/opportunities/` have the shape each source
//! answered with on 29 Sep 2026 (HN, Grants.gov, Reddit, Product Hunt and
//! the App Store were fetched live that day; names made neutral). SAM.gov
//! and GitHub could not be reached from where this was built -- SAM.gov
//! wants a key, and the proxy refuses GitHub's API -- so those two follow
//! their published documentation and say so.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::hunt::{self, Ask, Found, HuntConfig, HuntState, Interests, Kind, Said, Source};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn sample(name: &str) -> String {
    std::fs::read_to_string(format!("tests/fixtures/opportunities/{name}")).unwrap()
}

/// 29 Sep 2026, 12:00 UTC.
const NOW: u64 = 1_790_683_200;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-hunt-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn cfg_on() -> Config {
    let mut c = Config::load(Path::new("config")).unwrap();
    if let Some(t) = c.tools.as_mut() {
        t.hunt.enabled = true;
        // Any hour: the test's clock is noon UTC, which is early morning in
        // some zones.
        t.hunt.from_hour = 0;
    }
    c
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

/// Answers each request from the saved samples.
fn from_samples(a: &Ask) -> Result<String, String> {
    let (host, path) = match a {
        Ask::Get { host, path, .. } | Ask::PostJson { host, path, .. } => (host.as_str(), path.as_str()),
    };
    Ok(match (host, path) {
        ("hn.algolia.com", p) if p.starts_with("/api/v1/search_by_date") => sample("hn_search.json"),
        ("hn.algolia.com", p) if p.starts_with("/api/v1/items/") => sample("hn_thread.json"),
        ("api.grants.gov", _) => sample("grants.json"),
        ("api.sam.gov", _) => sample("sam.json"),
        ("www.reddit.com", p) if p.starts_with("/r/forhire") => sample("reddit_forhire.xml"),
        ("www.reddit.com", _) => "<?xml version=\"1.0\"?><feed xmlns=\"http://www.w3.org/2005/Atom\"><title>empty</title></feed>".into(),
        ("www.producthunt.com", _) => sample("producthunt.xml"),
        ("rss.marketingtools.apple.com", _) => sample("appstore_top_free.json"),
        ("api.github.com", _) => sample("github_search.json"),
        _ => return Err(format!("{host} isn't in the samples")),
    })
}

/// A day's read through the daemon's own tick -- its schedule, budget and
/// thread -- with the saved answers standing in for the network.
fn read_all(d: &mut Daemon, t: u64) -> HuntState {
    let none = std::time::Duration::ZERO;
    atlas::hunting::tick_with(d, t, true, from_samples, none);
    for _ in 0..500 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        atlas::hunting::tick_with(d, t + 1, true, from_samples, none);
        let s: HuntState = d.store.load(hunt::FILE);
        if s.sources.get("hn").map(|x| x.last_read > 0).unwrap_or(false) {
            return s;
        }
    }
    panic!("the read never came back");
}

fn want(words: &[&str], skills: &[&str]) -> Interests {
    Interests {
        want: words.iter().map(|s| s.to_string()).collect(),
        skills: skills.iter().map(|s| s.to_string()).collect(),
        avoid: vec![],
    }
}

// ---------------------------------------------------------------- the sources

#[test]
fn hn_finds_the_newest_hiring_and_freelance_threads_and_reads_one_posting_per_comment() {
    let threads = hunt::hn_threads(&sample("hn_search.json")).unwrap();
    assert!(threads.contains(&("49522897".to_string(), Kind::Job)), "{threads:?}");
    assert!(threads.contains(&("49522898".to_string(), Kind::Gig)), "{threads:?}");
    assert!(!threads.iter().any(|(id, _)| id == "49156683"), "August's thread is older than September's");
    assert!(!threads.iter().any(|(id, _)| id == "49522896"), "'who wants to be hired' is people, not openings");

    let posts = hunt::hn_postings(&sample("hn_thread.json"), Kind::Job).unwrap();
    assert_eq!(posts.len(), 3, "the chatty reply with no header is not a posting: {posts:#?}");
    let rust = &posts[0];
    assert!(rust.title.starts_with("Northwind Analytics | Senior Rust Engineer"), "{}", rust.title);
    assert_eq!(rust.link, "https://news.ycombinator.com/item?id=49522903");
    assert!(rust.remote);
    assert_eq!(rust.pay.as_deref(), Some("$150k–190k"));
    assert!(!rust.summary.contains("<p>") && !rust.summary.contains("&#x2F;"), "HTML and entities are gone: {}", rust.summary);
}

#[test]
fn grants_gov_answers_with_closing_dates_and_a_closed_grant_is_not_offered() {
    let g = hunt::grants_gov(&sample("grants.json"), NOW).unwrap();
    assert_eq!(g.len(), 3);
    let video = g.iter().find(|f| f.id == "grants:360101").unwrap();
    assert_eq!(video.title, "Small Business Video Production Training & Outreach");
    assert_eq!(video.kind, Kind::Grant);
    assert!(video.closes.is_some());
    assert_eq!(video.link, "https://www.grants.gov/search-results-detail/360101");
    let closed = g.iter().find(|f| f.id == "grants:329000").unwrap();
    assert!(hunt::freshness_of(closed, NOW).is_none(), "closed in 2020");
    assert!(hunt::grants_gov(r#"{"errorcode":5,"msg":"bad request"}"#, NOW).unwrap_err().contains("bad request"));
}

#[test]
fn sam_gov_parses_its_documented_shape_and_never_repeats_the_key() {
    let s = hunt::sam(&sample("sam.json"), NOW).unwrap();
    assert_eq!(s.len(), 2);
    assert_eq!(s[0].kind, Kind::Contract);
    assert!(s[0].closes.is_some() && s[0].link.starts_with("https://sam.gov/opp/"));

    let cfg = HuntConfig { sources: "sam".into(), ..Default::default() };
    let key = "SAM-SECRET-KEY-1234";
    let seen = std::cell::RefCell::new(Vec::new());
    let failing = |a: &Ask| -> Result<String, String> {
        seen.borrow_mut().push(a.clone());
        Err(format!("api.sam.gov refused {}", match a { Ask::Get { path, .. } => path.clone(), _ => String::new() }))
    };
    let took = hunt::read(Source::Sam, &cfg, &failing, key, "", NOW, (2026, 9, 29));
    let err = took.error.unwrap();
    assert!(!err.contains(key), "the key leaked into an error: {err}");
    let asked = seen.borrow();
    assert_eq!(asked.len(), 1, "one of its ten a day");
    assert!(!asked[0].shown().contains(key), "{}", asked[0].shown());
    // No key: no request at all.
    let none = hunt::read(Source::Sam, &cfg, &failing, "", "", NOW, (2026, 9, 29));
    assert_eq!(none.requests, 0);
    assert!(none.error.unwrap().contains("vault"));
}

#[test]
fn reddit_forhire_keeps_only_the_hiring_posts_with_clean_links() {
    let r = hunt::reddit(&sample("reddit_forhire.xml"), "forhire", NOW).unwrap();
    assert_eq!(r.len(), 2, "[FOR HIRE] is someone selling, not an opening: {r:#?}");
    assert!(r.iter().all(|f| f.title.to_lowercase().contains("[hiring]")));
    let rust = r.iter().find(|f| f.title.contains("Rust")).unwrap();
    assert!(!rust.link.contains("utm_"), "trackers taken off: {}", rust.link);
    let video = r.iter().find(|f| f.title.contains("video")).unwrap();
    assert!(video.summary.contains("$35/hour"), "{}", video.summary);
    assert!(video.remote);
}

#[test]
fn niches_from_product_hunt_the_app_store_github_and_a_search() {
    let ph = hunt::feed_items(&sample("producthunt.xml"), Source::ProductHunt, Kind::Niche, NOW).unwrap();
    assert_eq!(ph.len(), 2);
    assert_eq!(ph[0].title, "Northwind Clips");
    assert!(ph[0].summary.contains("Turn long video into shorts"));

    let apps = hunt::app_chart(&sample("appstore_top_free.json"), "top-free", NOW).unwrap();
    assert_eq!(apps.len(), 3);
    assert!(apps[1].title.contains("number 2 in top free apps"), "{}", apps[1].title);

    let gh = hunt::github(&sample("github_search.json"), NOW).unwrap();
    assert_eq!(gh.len(), 2);
    assert!(gh[0].title.contains("1840 stars"));
    assert!(hunt::github(r#"{"message":"API rate limit exceeded"}"#, NOW).unwrap_err().contains("rate limit"));

    let sx = hunt::search_results(&sample("searxng.json"), NOW).unwrap();
    assert_eq!(sx.len(), 1, "a javascript: link is never kept");
    assert!(!sx[0].link.contains("utm_"));
}

#[test]
fn job_alert_emails_already_in_the_inbox_count_and_nothing_else_does() {
    use atlas::mailbook::Letter;
    let letter = |id: &str, from: &str, subject: &str, excerpt: &str| Letter {
        id: id.into(),
        in_reply_to: None,
        refs: vec![],
        from_name: "Alerts".into(),
        from: from.into(),
        to: vec!["me@example.com".into()],
        subject: subject.into(),
        at: NOW - 3600,
        dated: true,
        mine: false,
        excerpt: excerpt.into(),
    };
    let letters = vec![
        letter("a", "donotreply@upwork.com", "New job: Short-form video editor", "Hourly $30-$45. Remote. https://www.upwork.com/jobs/~01abc?utm_source=alert"),
        letter("b", "jobs-noreply@linkedin.com", "Rust Engineer at Northwind", "See the job https://www.linkedin.com/jobs/view/123"),
        letter("c", "security@upwork.com", "Your Upwork verification code", "123456"),
        letter("d", "maya@quillbrook.example", "Lunch?", "Are you free Friday?"),
    ];
    let f = hunt::alerts_in_mail(&letters, NOW - 86_400);
    assert_eq!(f.len(), 2, "{f:#?}");
    assert_eq!(f[0].kind, Kind::Gig);
    assert_eq!(f[0].link, "https://www.upwork.com/jobs/~01abc");
    assert_eq!(f[1].kind, Kind::Job);
}

// ---------------------------------------------------------------- dedupe, filter, weigh

fn gig(id: &str, title: &str, link: &str) -> Found {
    Found {
        id: id.into(),
        source: Source::Reddit,
        kind: Kind::Gig,
        title: title.into(),
        link: link.into(),
        summary: "Remote, contract, $35/hour.".into(),
        at: NOW - 3600,
        closes: None,
        pay: Some("$35/hour".into()),
        remote: true,
    }
}

#[test]
fn the_same_thing_twice_or_cross_posted_is_offered_once() {
    let mut s = HuntState::default();
    let you = want(&["video"], &[]);
    let a = gig("r:1", "[Hiring] Short-form video editor, remote, $35/hr", "https://www.reddit.com/r/forhire/comments/1/x/");
    let again = gig("r:1", "[Hiring] Short-form video editor, remote, $35/hr", "https://www.reddit.com/r/forhire/comments/1/x/?utm_source=share");
    let cross = gig("r:2", "[HIRING] Short form video editor - remote - $35/hr", "https://www.reddit.com/r/slavelabour/comments/9/y/");
    let m = s.merge(vec![a, again, cross], &you, NOW);
    assert_eq!(m.new, 1, "{m:?}");
    assert_eq!(m.duplicates, 2, "{m:?}");
    // And again tomorrow: still once.
    let m2 = s.merge(vec![gig("r:1", "[Hiring] Short-form video editor, remote, $35/hr", "https://www.reddit.com/r/forhire/comments/1/x/")], &you, NOW + 3600);
    assert_eq!(m2.new, 0);
    assert_eq!(s.shortlist.len(), 1);
}

#[test]
fn what_you_want_filters_and_every_axis_says_what_it_rests_on() {
    let you = Interests { want: vec!["video".into()], skills: vec!["rust".into()], avoid: vec!["crypto".into()] };
    let nope = BTreeMap::new();
    let r = hunt::weigh_found(&gig("1", "[Hiring] Rust developer for a video tool", "https://x.example/1"), &you, &nope).unwrap();
    use atlas::opportunity::{Axis, Finding};
    match r.weighed.finding(Axis::Fit).unwrap() {
        Finding::Scored { rests_on, .. } => {
            assert!(rests_on.iter().any(|w| w.contains("rust")), "{rests_on:?}");
            assert!(rests_on.iter().any(|w| w.contains("video")), "{rests_on:?}");
        }
        other => panic!("fit should be scored: {other:?}"),
    }
    // Money is never guessed.
    assert!(matches!(r.weighed.finding(Axis::Money), Some(Finding::Blocked(_))));
    // Every scored axis rests on something.
    for l in &r.weighed.looks {
        if let Finding::Scored { rests_on, .. } = &l.finding {
            assert!(!rests_on.is_empty(), "{:?} scored on nothing", l.axis);
        }
    }
    assert!(hunt::weigh_found(&gig("2", "[Hiring] Crypto video promoter", "https://x.example/2"), &you, &nope).unwrap_err().contains("crypto"));
    assert!(hunt::weigh_found(&gig("3", "[Hiring] Bookkeeper", "https://x.example/3"), &you, &nope).unwrap_err().contains("matches"));
    // Nothing said yet: nothing filtered out, and Fit says why it can't judge.
    let open = hunt::weigh_found(&gig("4", "[Hiring] Bookkeeper", "https://x.example/4"), &Interests::default(), &nope).unwrap();
    assert!(matches!(open.weighed.finding(Axis::Fit), Some(Finding::Unknown(_))));
}

#[test]
fn not_interested_learns_so_the_next_one_like_it_is_held_back() {
    let mut s = HuntState::default();
    let you = Interests::default();
    s.merge(
        vec![
            gig("1", "[Hiring] Dropshipping store assistant", "https://x.example/1"),
            gig("2", "[Hiring] Dropshipping store manager", "https://x.example/2"),
        ],
        &you,
        NOW,
    );
    assert!(s.not_interested("1").is_some());
    assert!(s.not_interested("2").is_some());
    let m = s.merge(vec![gig("3", "[Hiring] Dropshipping store helper", "https://x.example/3")], &you, NOW);
    assert_eq!(m.new, 0, "two noes on 'dropshipping store' hold the third back");
    assert!(s.rejected.get("3").unwrap().contains("said no to"), "{:?}", s.rejected.get("3"));
    // And a no is for good.
    assert!(s.rejected.contains_key("1"));
    let m = s.merge(vec![gig("1", "[Hiring] Dropshipping store assistant", "https://x.example/other")], &you, NOW);
    assert_eq!(m.new, 0);
}

#[test]
fn gigs_go_stale_in_days_and_niches_last_weeks() {
    let mut g = gig("1", "x", "https://x.example/1");
    g.at = NOW - 4 * 86_400;
    assert!(hunt::freshness_of(&g, NOW).is_none(), "a four-day-old gig is gone");
    let mut n = g.clone();
    n.kind = Kind::Niche;
    assert!(hunt::freshness_of(&n, NOW).is_some(), "a four-day-old niche is not");
}

// ---------------------------------------------------------------- politeness

#[test]
fn a_whole_days_read_stays_well_under_the_budget_and_the_ceiling() {
    assert_eq!(HuntConfig { max_requests_per_day: 5000, ..Default::default() }.budget(), hunt::HARD_CEILING);
    let cfg = HuntConfig { sources: "hn, grants, sam, reddit, producthunt, appstore, github, mail".into(), ..Default::default() };
    let total: u32 = cfg.sources().iter().map(|s| hunt::cost(*s, &cfg, false)).sum();
    assert!(total <= 20, "a day's read is {total} requests; far inside 300");
    assert!(total <= cfg.budget());
    // Read once a day; HN weekly after the first week of the month.
    assert!(!hunt::source_due(Source::Grants, NOW - 3600, NOW, 29));
    assert!(hunt::source_due(Source::Grants, NOW - 86_400, NOW, 29));
    assert!(!hunt::source_due(Source::Hn, NOW - 2 * 86_400, NOW, 29));
    assert!(hunt::source_due(Source::Hn, NOW - 86_400, NOW, 3));
}

#[test]
fn the_tick_keeps_to_the_daily_budget_and_says_what_it_skipped() {
    let mut c = cfg_on();
    c.tools.as_mut().unwrap().hunt.max_requests_per_day = 4;
    let p = plat();
    let mut d = daemon(&c, &p, "budget");
    let s = read_all(&mut d, NOW);
    assert!(s.requests_today <= 4, "{} requests against a budget of 4", s.requests_today);
    let skipped: Vec<&String> = s.sources.iter().filter(|(_, st)| st.last_error.contains("requests a day")).map(|(k, _)| k).collect();
    assert!(!skipped.is_empty(), "something had to wait for tomorrow: {:?}", s.sources);
}

#[test]
fn the_tick_does_nothing_when_off() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = daemon(&c, &p, "tick-off");
    atlas::hunting::tick_with(&mut d, NOW, true, from_samples, std::time::Duration::ZERO);
    std::thread::sleep(std::time::Duration::from_millis(50));
    atlas::hunting::tick_with(&mut d, NOW + 1, true, from_samples, std::time::Duration::ZERO);
    let s: HuntState = d.store.load(hunt::FILE);
    assert_eq!(s.requests_today, 0);
    assert!(s.sources.is_empty());
}

#[test]
fn off_by_default_and_the_shipped_config_says_so() {
    assert!(!HuntConfig::default().enabled);
    let c = Config::load(Path::new("config")).unwrap();
    assert!(!c.tools.unwrap().hunt.enabled, "it reaches outside the machine: yours to turn on");
}

#[test]
fn the_hunter_never_sends_applies_or_spends() {
    // Read as text: nothing in either file can reach mail sending, the
    // outbox, messaging or a payment.
    for f in ["src/hunt.rs", "src/hunting.rs"] {
        let src = std::fs::read_to_string(f).unwrap();
        for never in ["smtp::", "outbox::", "messaging::", "outreach::", "orders::", "::send_", "publish::"] {
            assert!(!src.contains(never), "{f} reaches {never}");
        }
    }
}

// ---------------------------------------------------------------- talking to it

#[test]
fn what_you_say_to_it_is_read_as_whole_sentences() {
    assert_eq!(hunt::understand("Any opportunities?", 0), Some(Said::List));
    assert_eq!(hunt::understand("Atlas, what opportunities have you found", 0), Some(Said::List));
    assert_eq!(hunt::understand("look for opportunities in video editing, rust and grants", 0), Some(Said::LookFor(vec!["video editing".into(), "rust".into(), "grants".into()])));
    assert_eq!(hunt::understand("my skills are Rust, video editing", 0), Some(Said::Skills(vec!["rust".into(), "video editing".into()])));
    assert_eq!(hunt::understand("tell me more about 2", 3), Some(Said::More(1)));
    assert_eq!(hunt::understand("not interested in the first one", 3), Some(Said::NotInterested(0)));
    assert_eq!(hunt::understand("save opportunity 3", 3), Some(Said::Save(2)));
    // With no list showing, a number means nothing here.
    assert_eq!(hunt::understand("save 2", 0), None);
    // Out of range, and ordinary sentences, are not the hunter's.
    assert_eq!(hunt::understand("save 9", 3), None);
    assert_eq!(hunt::understand("what opportunities are there in rust as a language", 0), None);
    assert_eq!(hunt::understand("open chrome", 3), None);
    // 29 Sep 2026, the shapes Eric used: a kind of work, and a thing to skip.
    assert_eq!(hunt::understand("look for video editing gigs", 0), Some(Said::LookFor(vec!["video editing".into()])));
    assert_eq!(hunt::understand("find me rust or go contracts", 0), Some(Said::LookFor(vec!["rust".into(), "go".into()])));
    assert_eq!(hunt::understand("not interested in crypto gigs", 0), Some(Said::Avoid(vec!["crypto".into()])));
    // With a list showing, the kind of work need not be said.
    assert_eq!(hunt::understand("not interested in crypto", 3), Some(Said::Avoid(vec!["crypto".into()])));
    assert_eq!(hunt::understand("I'm not interested in the meeting", 0), None, "not the hunter's with no list and no kind of work");
    assert_eq!(hunt::understand("tell me more about #3", 3), Some(Said::More(2)));
    assert_eq!(hunt::understand("not interested in that kind", 3), Some(Said::NotThatKind(hunt::THAT_ONE)));
    assert_eq!(hunt::understand("no more like 2", 3), Some(Said::NotThatKind(1)));
    assert_eq!(hunt::understand("not interested in that one", 3), Some(Said::NotInterested(hunt::THAT_ONE)));
    // Not everything that starts the same way.
    assert_eq!(hunt::understand("look for my keys", 0), None);
    assert_eq!(hunt::understand("look for more work", 0), None);
    assert_eq!(hunt::understand("no more jokes", 0), None);
}

#[test]
fn the_parser_sends_the_hunters_sentences_to_its_own_command() {
    use atlas::intent::{Intent, Parser};
    let c = Config::load(Path::new("config")).unwrap();
    let parser = Parser::new(&c.commands);
    for s in ["any opportunities?", "look for video editing gigs", "my skills are rust and video", "not interested in crypto gigs", "start hunting for opportunities"] {
        assert!(matches!(parser.parse(s), Intent::Opportunities(_)), "{s}: {:?}", parser.parse(s));
    }
    // Numbers only mean the list while it's the one showing.
    assert!(!matches!(parser.parse("save 2"), Intent::Opportunities(_)));
    let mut p2 = parser.clone();
    p2.know_workday(atlas::workday::Known { follow: Some(atlas::workday::Follow::Opportunities(3)), ..Default::default() });
    assert!(matches!(p2.parse("save 2"), Intent::Opportunities(_)));
    assert!(matches!(p2.parse("tell me more about #3"), Intent::Opportunities(_)));
    assert!(matches!(p2.parse("not interested in crypto"), Intent::Opportunities(_)));
    // A web search stays a web search.
    assert!(!matches!(parser.parse("search for rust jobs"), Intent::Opportunities(_)));
}

#[test]
fn that_kind_holds_back_the_next_one_like_it_straight_away() {
    let mut s = HuntState::default();
    let you = Interests::default();
    s.merge(vec![gig("1", "[Hiring] Dropshipping store assistant", "https://x.example/1")], &you, NOW);
    assert!(s.not_that_kind("1").is_some());
    let m = s.merge(vec![gig("2", "[Hiring] Dropshipping store helper", "https://x.example/2")], &you, NOW);
    assert_eq!(m.new, 0, "one 'that kind' is enough, where one 'not interested' is not");
    let m = s.merge(vec![gig("3", "[Hiring] Podcast video editor", "https://x.example/3")], &you, NOW);
    assert_eq!(m.new, 1, "and nothing else is held back");
}

#[test]
fn the_list_by_voice_tell_me_more_not_interested_and_save() {
    let c = cfg_on();
    let p = plat();
    let mut d = daemon(&c, &p, "voice");
    // Asked once what to look for, and the answer kept as facts you stated.
    let r = d.turn("look for opportunities in video, rust", NOW);
    assert!(r.contains("video, rust"), "{r}");
    assert!(d.facts.get(hunt::FACT_WANT).unwrap().kind.came_from_you());

    let read = read_all(&mut d, NOW);
    for (name, st) in &read.sources {
        assert!(st.last_error.is_empty(), "{name}: {}", st.last_error);
    }
    assert!(read.requests_today > 0 && read.requests_today <= 20, "{} requests", read.requests_today);
    // Read again the same day: nothing is due, nothing is asked.
    atlas::hunting::tick_with(&mut d, NOW + 7200, true, from_samples, std::time::Duration::ZERO);
    let again: HuntState = d.store.load(hunt::FILE);
    assert_eq!(again.requests_today, read.requests_today, "once a day per source");

    let list = d.turn("any opportunities?", NOW + 10);
    assert!(list.starts_with("1. "), "{list}");
    assert!(list.to_lowercase().contains("video") || list.to_lowercase().contains("rust"), "{list}");
    assert!(!list.contains("Bookkeeper"));

    let more = d.turn("tell me more about 1", NOW + 20);
    assert!(more.contains("haven't applied"), "{more}");
    assert!(more.contains("The money: not yet"), "money is never guessed: {more}");

    let saved = d.turn("save 1", NOW + 30);
    assert!(saved.starts_with("Saved"), "{saved}");
    let dropped = d.turn("not interested in 1", NOW + 40);
    assert!(dropped.starts_with("Dropped"), "{dropped}");

    // Kept across a restart.
    let kept: HuntState = d.store.load(hunt::FILE);
    assert_eq!(kept.saved.len(), 1);
    assert!(kept.rejected.values().any(|w| w == "you said not interested"));

    // Said out loud, a kind of work is added to what was there, not put in
    // its place; a thing to skip comes off what you look for.
    let r = d.turn("look for video editing gigs", NOW + 50);
    assert!(r.contains("video editing"), "{r}");
    assert_eq!(atlas::hunt::Interests::from_facts(&d.facts).want, vec!["video".to_string(), "rust".into(), "video editing".into()]);
    let r = d.turn("not interested in rust gigs", NOW + 60);
    assert!(r.contains("rust"), "{r}");
    let you = atlas::hunt::Interests::from_facts(&d.facts);
    assert_eq!(you.avoid, vec!["rust".to_string()]);
    assert!(!you.want.contains(&"rust".to_string()), "{:?}", you.want);
}

#[test]
fn tell_me_more_then_that_kind_by_voice() {
    let c = cfg_on();
    let p = plat();
    let mut d = daemon(&c, &p, "thatkind");
    d.turn("my skills are video", NOW);
    read_all(&mut d, NOW);
    let list = d.turn("any opportunities?", NOW + 10);
    assert!(list.starts_with("1. ") && list.contains("\n2. "), "{list}");
    let second = list.lines().nth(1).unwrap().to_string();
    let more = d.turn("tell me more about #2", NOW + 20);
    assert!(more.contains("haven't applied"), "{more}");
    let r = d.turn("not interested in that kind", NOW + 30);
    assert!(r.contains("anything like it"), "{r}");
    let title = second.trim_start_matches("2. ").split(" — ").next().unwrap();
    assert!(r.contains(title), "the one talked about, not the first: {r} / {second}");
    let kept: HuntState = d.store.load(hunt::FILE);
    assert!(kept.nope.values().any(|n| *n >= 2), "{:?}", kept.nope);
    // The list stays live for the numbers left on it.
    let r = d.turn("save 2", NOW + 40);
    assert!(r.starts_with("Saved"), "still the hunter's list: {r}");
}

#[test]
fn the_morning_brief_carries_the_best_few_with_why_and_asks_once() {
    let c = cfg_on();
    let p = plat();
    let mut d = daemon(&c, &p, "brief");
    // Nothing known yet: the brief asks, once.
    let first = atlas::hunting::brief_items(&mut d, NOW);
    assert!(first.iter().any(|i| i.subject.contains("What kinds of opportunities")), "{first:?}");
    let second = atlas::hunting::brief_items(&mut d, NOW + 60);
    assert!(!second.iter().any(|i| i.subject.contains("What kinds of opportunities")), "asked once, not every morning");

    d.turn("my skills are video", NOW);
    read_all(&mut d, NOW);
    let items = atlas::hunting::brief_items(&mut d, NOW + 120);
    assert_eq!(items.len(), 3, "top_n as shipped: {items:#?}");
    assert!(items.iter().all(|i| i.subject.contains("uses your video")), "each says why: {items:#?}");

    // 5 Oct 2026 (Eric: "the opportunities are spamming me"): the brief is
    // built for the morning, every part-of-day hello and every welcome
    // back. A find is volunteered once; the next brief has none of them.
    let again = atlas::hunting::brief_items(&mut d, NOW + 3600);
    let said_before: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
    assert!(again.iter().all(|i| !said_before.contains(&i.id.as_str())), "the same finds said again: {again:#?}");
    // Asking still lists them all.
    let asked = d.turn("any opportunities?", NOW + 3700);
    assert!(asked.contains("1."), "asking lists them: {asked}");
}

#[test]
fn the_brief_says_nothing_when_hunting_is_off() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = daemon(&c, &p, "off");
    assert!(atlas::hunting::brief_items(&mut d, NOW).is_empty());
}

#[test]
fn the_opportunities_page_lists_them_and_its_buttons_work() {
    use atlas::hub::Page;
    use atlas::server::Action;
    let c = cfg_on();
    let p = plat();
    let mut d = daemon(&c, &p, "page");
    // The page reads the real clock, so what it shows is dated from it.
    let now = atlas::store::now();
    let mut s = HuntState::default();
    let mut a = gig("page:1", "[Hiring] Video editor <script>alert(1)</script>", "https://x.example/1");
    a.at = now - 600;
    let mut b = gig("page:2", "[Hiring] Rust developer, video tool", "https://x.example/2");
    b.at = now - 600;
    s.merge(vec![a, b], &Interests::default(), now);
    d.store.save(hunt::FILE, &s).unwrap();

    let html = atlas::hublive::reply(&mut d, Action::Hub(Page::Opportunities)).body;
    assert!(html.contains("<main id=main"), "rendered");
    assert!(html.contains("Worth a look") && html.contains("Not interested") && html.contains("Save it"));
    assert!(html.contains("Grants.gov") && html.contains("Hacker News hiring threads"), "each source's state is shown");
    assert!(!html.contains("<script>alert"), "listing text is escaped");
    assert!(html.contains("Video editor &lt;script&gt;"), "and still shown");

    let back = atlas::hublive::reply(&mut d, Action::HubPost { path: "/hub/opportunities".into(), fields: vec![("what".into(), "save".into()), ("id".into(), "page:2".into())] });
    assert!(back.status >= 300 && back.status < 400, "back to the page: {}", back.status);
    let kept: HuntState = d.store.load(hunt::FILE);
    assert!(kept.saved.iter().any(|f| f.id == "page:2"));

    // "Tell me more" on the page opens the detail in place.
    let more = atlas::hublive::reply(&mut d, Action::HubQ(Page::Opportunities, "more=page%3A1".into())).body;
    assert!(more.contains("haven&#x27;t applied") || more.contains("haven't applied") || more.contains("haven&#39;t applied"), "{}", &more[more.len().saturating_sub(3000)..]);

    // Interests set from the page are facts you stated.
    atlas::hublive::reply(&mut d, Action::HubPost {
        path: "/hub/opportunities".into(),
        fields: vec![("what".into(), "interests".into()), ("want".into(), "grants, video".into()), ("skills".into(), "rust".into()), ("avoid".into(), "".into())],
    });
    assert_eq!(Interests::from_facts(&d.facts).skills, vec!["rust".to_string()]);
    assert!(d.facts.get(hunt::FACT_SKILLS).unwrap().kind.came_from_you());
}
