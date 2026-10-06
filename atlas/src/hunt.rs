//! Opportunity hunting: gigs, jobs, grants, contracts and niches, found for
//! you on a polite daily schedule and brought as a short list with reasons.
//!
//! Not trading, not markets. Money here means "what it pays", and the money
//! axis stays `opportunity::money_is_not_auditable_yet` -- the hunter never
//! pretends to know what a thing is worth to you.
//!
//! **Sources** (research, 29 Sep 2026, each checked live that day):
//! - HN "Who is hiring?" and "Freelancer? Seeking freelancer?" through the
//!   Algolia API: the newest thread by `whoishiring`, then its comments. Each
//!   top-level comment is one posting, its first line a `|`-separated header.
//! - Grants.gov `search2` (POST, JSON, no key).
//! - SAM.gov opportunities with a free personal key kept in the vault. Ten
//!   requests a day for a key with no role; this uses one.
//! - Reddit subreddit feeds (`/r/<sub>/new/.rss`). r/forhire keeps only
//!   `[Hiring]` posts.
//! - Product Hunt's Atom feed and the App Store top charts: niches, not gigs.
//! - GitHub's search API for new repositories gathering stars (the trending
//!   page has no API).
//! - Any feed or SearXNG search you add, and job-alert emails already in your
//!   inbox (Upwork's RSS was switched off in 2024; its alerts by email are
//!   the one legitimate route left).
//!
//! **Politeness.** Each source is read at most once a local day (HN daily in
//! the first week of a month, weekly after), requests are spaced out, and a
//! day's total is capped at your setting and never above `HARD_CEILING`.
//! Off by default.
//!
//! **What it never does.** Apply, reply, contact anyone, sign up, or spend.
//! It reads public listings and your own mail, and it tells you. The only
//! things it keeps are what it found, what you saved, and what you said no
//! to -- so the same thing never comes back.
//!
//! Pure: every function here takes what it needs and returns what it found.
//! The daemon's side (fetching, the tick, the brief, the hub, what you say)
//! is `hunting.rs`.

use crate::opportunity::{Axis, Finding, Opportunity, Verdict};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

/// No setting takes a day above this many requests.
pub const HARD_CEILING: u32 = 300;
/// The most kept in each remembered list, oldest dropped first.
pub const KEEP_SEEN: usize = 5000;
pub const KEEP_SHORTLIST: usize = 200;
pub const KEEP_REJECTED: usize = 2000;
/// Near-duplicate: normalised-title trigram overlap at or above this.
pub const NEAR_DUPLICATE: f32 = 0.8;
/// Who is asking, sent with every request. Reddit refuses a bare request.
pub const USER_AGENT: &str = "Atlas/1.0 (a personal assistant reading public listings once a day for one person)";

// ---------------------------------------------------------------- settings

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HuntConfig {
    /// Off until you turn it on.
    pub enabled: bool,
    /// Which sources, by name, comma-separated: hn, grants, sam, reddit,
    /// producthunt, appstore, github, mail, feeds, search.
    pub sources: String,
    /// Subreddits read when `reddit` is on, comma-separated, without "r/".
    pub subreddits: String,
    /// Extra RSS or Atom feeds, comma-separated.
    pub feeds: String,
    /// SearXNG searches, comma-separated, when `research.searxng_url` is set.
    pub searches: String,
    /// What Grants.gov and SAM.gov are asked for.
    pub keywords: String,
    /// The vault entry holding your SAM.gov key.
    pub sam_key_vault: String,
    /// How many make the brief.
    pub top_n: u32,
    /// Requests a day, all sources together. Never above `HARD_CEILING`.
    pub max_requests_per_day: u32,
    /// The earliest local hour it goes looking, so the list is ready for
    /// the morning brief.
    pub from_hour: u32,
}

impl Default for HuntConfig {
    fn default() -> Self {
        HuntConfig {
            enabled: false,
            sources: "hn, grants, reddit, producthunt, appstore, github, mail".into(),
            subreddits: "forhire, SideProject, Entrepreneur, smallbusiness".into(),
            feeds: String::new(),
            searches: String::new(),
            keywords: "small business".into(),
            sam_key_vault: "sam.gov".into(),
            top_n: 3,
            max_requests_per_day: 60,
            from_hour: 5,
        }
    }
}

/// A comma list, trimmed, empties dropped.
pub fn list(s: &str) -> Vec<String> {
    s.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect()
}

impl HuntConfig {
    pub fn budget(&self) -> u32 {
        self.max_requests_per_day.min(HARD_CEILING)
    }

    pub fn sources(&self) -> Vec<Source> {
        let mut out: Vec<Source> = list(&self.sources).iter().filter_map(|s| Source::named(s)).collect();
        out.dedup();
        out
    }
}

// ---------------------------------------------------------------- sources

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Hn,
    Grants,
    Sam,
    Reddit,
    ProductHunt,
    AppStore,
    Github,
    Mail,
    Feeds,
    Search,
}

pub const ALL_SOURCES: &[Source] = &[
    Source::Hn,
    Source::Grants,
    Source::Sam,
    Source::Reddit,
    Source::ProductHunt,
    Source::AppStore,
    Source::Github,
    Source::Mail,
    Source::Feeds,
    Source::Search,
];

impl Source {
    pub fn named(s: &str) -> Option<Source> {
        let t: String = s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect();
        Some(match t.as_str() {
            "hn" | "hackernews" | "whoishiring" => Source::Hn,
            "grants" | "grantsgov" => Source::Grants,
            "sam" | "samgov" => Source::Sam,
            "reddit" => Source::Reddit,
            "producthunt" | "ph" => Source::ProductHunt,
            "appstore" | "apps" => Source::AppStore,
            "github" | "githubtrending" => Source::Github,
            "mail" | "email" | "inbox" => Source::Mail,
            "feeds" | "rss" => Source::Feeds,
            "search" | "searxng" => Source::Search,
            _ => return None,
        })
    }

    pub fn key(self) -> &'static str {
        match self {
            Source::Hn => "hn",
            Source::Grants => "grants",
            Source::Sam => "sam",
            Source::Reddit => "reddit",
            Source::ProductHunt => "producthunt",
            Source::AppStore => "appstore",
            Source::Github => "github",
            Source::Mail => "mail",
            Source::Feeds => "feeds",
            Source::Search => "search",
        }
    }

    pub fn plain(self) -> &'static str {
        match self {
            Source::Hn => "Hacker News hiring threads",
            Source::Grants => "Grants.gov",
            Source::Sam => "SAM.gov contracts",
            Source::Reddit => "Reddit",
            Source::ProductHunt => "Product Hunt",
            Source::AppStore => "App Store charts",
            Source::Github => "new on GitHub",
            Source::Mail => "job alerts in your mail",
            Source::Feeds => "your feeds",
            Source::Search => "your searches",
        }
    }

    /// How far a source is taken at its word. Rests on who can post there.
    fn standing(self) -> (u8, &'static str) {
        match self {
            Source::Grants => (8, "a federal listing on Grants.gov"),
            Source::Sam => (8, "a federal listing on SAM.gov"),
            Source::Hn => (7, "posted in Hacker News's monthly hiring thread, where each company posts itself"),
            Source::Mail => (6, "an alert from a job site you signed up to"),
            Source::ProductHunt => (5, "launched on Product Hunt: shows people are building here, not that it pays"),
            Source::AppStore => (6, "in the App Store's own top chart: real downloads, not a pitch"),
            Source::Github => (5, "new repositories gathering stars: attention, not money"),
            Source::Reddit => (4, "a Reddit post: anyone can post, nothing is checked"),
            Source::Feeds => (5, "from a feed you chose"),
            Source::Search => (4, "a search result: only as good as the page"),
        }
    }
}

/// What sort of thing it is. Decides how fast it goes stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Short paid work.
    Gig,
    /// A job opening.
    Job,
    Grant,
    /// A government contract.
    Contract,
    /// Something being built or bought: a gap worth a look.
    Niche,
    /// A lead from a feed or a search, not yet sorted.
    Lead,
}

impl Kind {
    pub fn plain(self) -> &'static str {
        match self {
            Kind::Gig => "gig",
            Kind::Job => "job",
            Kind::Grant => "grant",
            Kind::Contract => "contract",
            Kind::Niche => "niche",
            Kind::Lead => "lead",
        }
    }

    /// (half-life, dropped after), in hours. Gigs go in two or three days;
    /// niches last weeks; a grant or contract lasts until it closes.
    fn shelf(self) -> (u64, u64) {
        match self {
            Kind::Gig => (48, 72),
            Kind::Job => (7 * 24, 30 * 24),
            Kind::Lead => (3 * 24, 7 * 24),
            Kind::Niche => (14 * 24, 30 * 24),
            Kind::Grant | Kind::Contract => (30 * 24, 90 * 24),
        }
    }
}

/// One thing found, normalised.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Found {
    /// Stable: the source's own id where it has one, else the clean link.
    pub id: String,
    pub source: Source,
    pub kind: Kind,
    pub title: String,
    pub link: String,
    /// Plain text, trimmed.
    pub summary: String,
    /// When it was posted (UTC seconds), or when it was found.
    pub at: u64,
    /// When applications close, where the listing says.
    #[serde(default)]
    pub closes: Option<u64>,
    /// Pay, as the listing states it. Never worked out.
    #[serde(default)]
    pub pay: Option<String>,
    #[serde(default)]
    pub remote: bool,
}

// ---------------------------------------------------------------- requests

/// One request. `secret` marks a query string holding a key, so no error or
/// log line ever repeats the path.
#[derive(Debug, Clone, PartialEq)]
pub enum Ask {
    Get { https: bool, host: String, path: String, secret: bool },
    PostJson { host: String, path: String, body: String },
}

impl Ask {
    fn get(host: &str, path: &str) -> Ask {
        Ask::Get { https: true, host: host.into(), path: path.into(), secret: false }
    }

    pub fn host(&self) -> &str {
        match self {
            Ask::Get { host, .. } | Ask::PostJson { host, .. } => host,
        }
    }

    /// For a status line: the host, and the path unless it holds a key.
    pub fn shown(&self) -> String {
        match self {
            Ask::Get { host, secret: true, .. } => format!("{host} (key withheld)"),
            Ask::Get { host, path, .. } | Ask::PostJson { host, path, .. } => format!("{host}{path}"),
        }
    }
}

/// Does the request; `Ok(body)` on a 2xx.
pub type Fetch<'a> = &'a dyn Fn(&Ask) -> Result<String, String>;

/// What one source brought back.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Took {
    pub found: Vec<Found>,
    pub requests: u32,
    pub error: Option<String>,
}

/// The most requests a source can make in one read.
pub fn cost(source: Source, cfg: &HuntConfig, searxng: bool) -> u32 {
    match source {
        Source::Hn => 3,
        Source::Reddit => list(&cfg.subreddits).len() as u32,
        Source::Feeds => list(&cfg.feeds).len() as u32,
        Source::Search => if searxng { list(&cfg.searches).len() as u32 } else { 0 },
        Source::AppStore => 2,
        Source::Mail => 0,
        _ => 1,
    }
}

/// Is this source due today? `last` is when it was last read (0: never);
/// `day` and `day_of_month` are local.
///
/// Named `source_due` rather than `due` (29 Sep 2026): a second free `due`
/// beside `brief::due` made both ambiguous to the deadness scan
/// (tests/one_word_is_not_an_address.rs).
pub fn source_due(source: Source, last: u64, now: u64, day_of_month: u32) -> bool {
    if last == 0 {
        return true;
    }
    let gap = now.saturating_sub(last);
    match source {
        // New threads land on the first weekday of the month: daily while
        // they fill up, weekly after.
        Source::Hn if day_of_month > 7 => gap >= 7 * 86_400 - 3600,
        // Your own mail costs nothing to read again.
        Source::Mail => gap >= 3600,
        _ => gap >= 20 * 3600,
    }
}

fn scrub(e: String, key: &str) -> String {
    if key.is_empty() { e } else { e.replace(key, "…") }
}

/// Read one source. `sam_key` is only used for SAM.gov and never appears in
/// what comes back. `searxng` is the base address, or empty.
pub fn read(source: Source, cfg: &HuntConfig, fetch: Fetch, sam_key: &str, searxng: &str, now: u64, today: (i64, u32, u32)) -> Took {
    let mut took = Took::default();
    let mut ask = |a: Ask| -> Result<String, String> {
        took.requests += 1;
        fetch(&a).map_err(|e| scrub(e, sam_key))
    };
    let r: Result<Vec<Found>, String> = (|| match source {
        Source::Hn => {
            let list = ask(Ask::get("hn.algolia.com", "/api/v1/search_by_date?tags=story,author_whoishiring&hitsPerPage=10"))?;
            let threads = hn_threads(&list)?;
            let mut out = Vec::new();
            for (id, kind) in threads {
                let body = ask(Ask::get("hn.algolia.com", &format!("/api/v1/items/{id}")))?;
                out.extend(hn_postings(&body, kind)?);
            }
            Ok(out)
        }
        Source::Grants => {
            let body = serde_json::json!({ "keyword": cfg.keywords.trim(), "rows": 25, "oppStatuses": "forecasted|posted" }).to_string();
            let reply = ask(Ask::PostJson { host: "api.grants.gov".into(), path: "/v1/api/search2".into(), body })?;
            grants_gov(&reply, now)
        }
        Source::Sam => {
            if sam_key.trim().is_empty() {
                return Err(format!("no SAM.gov key in the vault under \"{}\"", cfg.sam_key_vault));
            }
            let (y, m, d) = today;
            let (fy, fm, fd) = crate::hubpages::ymd(crate::civil::days_from_civil(y, m, d) - 7);
            let path = format!(
                "/opportunities/v2/search?api_key={}&limit=25&postedFrom={fm:02}/{fd:02}/{fy}&postedTo={m:02}/{d:02}/{y}&title={}",
                crate::research::urlencode(sam_key.trim()),
                crate::research::urlencode(cfg.keywords.trim())
            );
            let reply = ask(Ask::Get { https: true, host: "api.sam.gov".into(), path, secret: true })?;
            sam(&reply, now)
        }
        Source::Reddit => {
            let mut out = Vec::new();
            let mut last_err = None;
            for sub in list(&cfg.subreddits) {
                let sub: String = sub.trim_start_matches("r/").chars().filter(|c| c.is_alphanumeric() || *c == '_').collect();
                if sub.is_empty() {
                    continue;
                }
                match ask(Ask::get("www.reddit.com", &format!("/r/{sub}/new/.rss"))) {
                    Ok(xml) => out.extend(reddit(&xml, &sub, now)?),
                    Err(e) => last_err = Some(format!("r/{sub}: {e}")),
                }
            }
            match (out.is_empty(), last_err) {
                (true, Some(e)) => Err(e),
                _ => Ok(out),
            }
        }
        Source::ProductHunt => {
            let xml = ask(Ask::get("www.producthunt.com", "/feed"))?;
            feed_items(&xml, Source::ProductHunt, Kind::Niche, now)
        }
        Source::AppStore => {
            let mut out = Vec::new();
            for chart in ["top-free", "top-paid"] {
                let body = ask(Ask::get("rss.marketingtools.apple.com", &format!("/api/v2/us/apps/{chart}/25/apps.json")))?;
                out.extend(app_chart(&body, chart, now)?);
            }
            Ok(out)
        }
        Source::Github => {
            let (y, m, d) = today;
            let (wy, wm, wd) = crate::hubpages::ymd(crate::civil::days_from_civil(y, m, d) - 7);
            let body = ask(Ask::get(
                "api.github.com",
                &format!("/search/repositories?q=created:%3E{wy}-{wm:02}-{wd:02}&sort=stars&order=desc&per_page=20"),
            ))?;
            github(&body, now)
        }
        Source::Feeds => {
            let mut out = Vec::new();
            for url in list(&cfg.feeds) {
                let Some((https, host, path)) = crate::feeds::split_feed_url(&url) else { continue };
                let xml = ask(Ask::Get { https, host, path, secret: false })?;
                out.extend(feed_items(&xml, Source::Feeds, Kind::Lead, now)?);
            }
            Ok(out)
        }
        Source::Search => {
            if searxng.trim().is_empty() {
                return Err("no SearXNG address is set (Settings, Reaching outside this machine)".into());
            }
            let mut out = Vec::new();
            for q in list(&cfg.searches) {
                let url = crate::research::searxng_url(searxng, &q);
                let Some((https, host, path)) = crate::feeds::split_feed_url(&url) else {
                    return Err(format!("{searxng} isn't a web address"));
                };
                let body = ask(Ask::Get { https, host, path, secret: false })?;
                out.extend(search_results(&body, now)?);
            }
            Ok(out)
        }
        // Read from the mail already on this machine, not fetched here.
        Source::Mail => Ok(Vec::new()),
    })();
    match r {
        Ok(f) => took.found = f,
        Err(e) => took.error = Some(scrub(e, sam_key)),
    }
    took
}

// ---------------------------------------------------------------- parsers

fn clip(s: &str, n: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= n {
        s
    } else {
        format!("{}…", s.chars().take(n).collect::<String>().trim_end())
    }
}

/// HTML to plain text, entities decoded.
fn plain_text(html: &str) -> String {
    let spaced = html.replace("<p>", "\n").replace("<br>", "\n");
    crate::feeds::text_of(&spaced)
}

/// Pay, as written: "$150 - 210K", "€75k–110k", "USD 15/hour".
fn pay_in(text: &str) -> Option<String> {
    let t = text;
    for (i, c) in t.char_indices() {
        if matches!(c, '$' | '€' | '£') {
            let rest: String = t[i..].chars().take(28).collect();
            let end = rest
                .char_indices()
                .skip(1)
                .find(|(_, ch)| !(ch.is_ascii_digit() || matches!(ch, ',' | '.' | 'k' | 'K' | ' ' | '-' | '–' | '$' | '€' | '£' | '/' | 'h' | 'r' | 'o' | 'u')))
                .map(|(j, _)| j)
                .unwrap_or(rest.len());
            let got = rest[..end].trim().trim_end_matches(['-', '–', '/', ' ']).to_string();
            if got.chars().any(|c| c.is_ascii_digit()) {
                return Some(got);
            }
        }
    }
    let low = t.to_ascii_lowercase();
    for cur in ["usd ", "eur ", "gbp "] {
        if let Some(i) = low.find(cur) {
            let rest: String = t[i..].chars().take(20).collect();
            if rest.chars().any(|c| c.is_ascii_digit()) {
                return Some(rest.split('|').next().unwrap_or("").trim().to_string());
            }
        }
    }
    None
}

/// The newest "Who is hiring?" and "Seeking freelancer?" threads.
pub fn hn_threads(json: &str) -> Result<Vec<(String, Kind)>, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("HN's answer wasn't JSON: {e}"))?;
    let hits = v["hits"].as_array().ok_or("HN's answer had no hits")?;
    let mut best: BTreeMap<&str, (i64, String)> = BTreeMap::new();
    for h in hits {
        let title = h["title"].as_str().unwrap_or("");
        let kind = if title.contains("Who is hiring?") {
            "hiring"
        } else if title.contains("Seeking freelancer?") {
            "freelance"
        } else {
            continue;
        };
        let at = h["created_at_i"].as_i64().unwrap_or(0);
        let id = h["objectID"].as_str().unwrap_or("").to_string();
        if id.is_empty() {
            continue;
        }
        if best.get(kind).map(|(t, _)| at > *t).unwrap_or(true) {
            best.insert(kind, (at, id));
        }
    }
    Ok(best
        .into_iter()
        .map(|(k, (_, id))| (id, if k == "freelance" { Kind::Gig } else { Kind::Job }))
        .collect())
}

/// One posting per top-level comment. The first line is the header:
/// "Company | Role | Remote | Full-time | $150k".
pub fn hn_postings(json: &str, kind: Kind) -> Result<Vec<Found>, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("HN's thread wasn't JSON: {e}"))?;
    let kids = v["children"].as_array().ok_or("HN's thread had no comments")?;
    let mut out = Vec::new();
    for c in kids {
        let Some(html) = c["text"].as_str() else { continue };
        let id = c["id"].as_i64().map(|i| i.to_string()).unwrap_or_default();
        if id.is_empty() || html.trim().is_empty() {
            continue;
        }
        let head_html = html.split("<p>").next().unwrap_or(html);
        let head = plain_text(head_html);
        let body = plain_text(html);
        // A reply that isn't a posting ("Is this still open?") has no header.
        if kind == Kind::Job && !head.contains('|') {
            continue;
        }
        out.push(Found {
            id: format!("hn:{id}"),
            source: Source::Hn,
            kind,
            title: clip(&head, 140),
            link: format!("https://news.ycombinator.com/item?id={id}"),
            summary: clip(&body, 600),
            at: c["created_at_i"].as_u64().unwrap_or(0),
            closes: None,
            pay: pay_in(&head),
            remote: head.to_lowercase().contains("remote"),
        });
    }
    Ok(out)
}

/// "12/14/2021" as UTC seconds at midnight.
fn mdy(s: &str) -> Option<u64> {
    let p: Vec<&str> = s.trim().split('/').collect();
    if p.len() != 3 {
        return None;
    }
    let (m, d, y) = (p[0].parse::<u32>().ok()?, p[1].parse::<u32>().ok()?, p[2].parse::<i64>().ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let days = crate::civil::days_from_civil(y, m, d);
    (days >= 0).then(|| days as u64 * 86_400)
}

pub fn grants_gov(json: &str, now: u64) -> Result<Vec<Found>, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("Grants.gov's answer wasn't JSON: {e}"))?;
    if v["errorcode"].as_i64().unwrap_or(0) != 0 {
        return Err(format!("Grants.gov said: {}", v["msg"].as_str().unwrap_or("an error")));
    }
    let hits = v["data"]["oppHits"].as_array().ok_or("Grants.gov's answer had no results list")?;
    Ok(hits
        .iter()
        .filter_map(|h| {
            let id = h["id"].as_str()?.to_string();
            let title = crate::feeds::text_of(h["title"].as_str()?);
            let agency = h["agency"].as_str().unwrap_or("");
            let status = h["oppStatus"].as_str().unwrap_or("");
            let closes = h["closeDate"].as_str().and_then(mdy);
            Some(Found {
                id: format!("grants:{id}"),
                source: Source::Grants,
                kind: Kind::Grant,
                title: clip(&title, 140),
                link: format!("https://www.grants.gov/search-results-detail/{id}"),
                summary: clip(&format!("{agency}. {} {}. Number {}.", status, h["docType"].as_str().unwrap_or(""), h["number"].as_str().unwrap_or("")), 300),
                at: h["openDate"].as_str().and_then(mdy).unwrap_or(now),
                closes,
                pay: None,
                remote: true,
            })
        })
        .collect())
}

/// "2026-10-15T17:00:00-04:00" or "2026-09-22" as UTC seconds, to the day.
fn iso_day(s: &str) -> Option<u64> {
    let d = s.get(..10)?;
    let p: Vec<&str> = d.split('-').collect();
    if p.len() != 3 {
        return None;
    }
    let days = crate::civil::days_from_civil(p[0].parse().ok()?, p[1].parse().ok()?, p[2].parse().ok()?);
    (days >= 0).then(|| days as u64 * 86_400)
}

pub fn sam(json: &str, now: u64) -> Result<Vec<Found>, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|_| "SAM.gov's answer wasn't JSON".to_string())?;
    if let Some(e) = v["error"]["message"].as_str().or(v["message"].as_str()) {
        return Err(format!("SAM.gov said: {e}"));
    }
    let data = v["opportunitiesData"].as_array().ok_or("SAM.gov's answer had no opportunities list")?;
    Ok(data
        .iter()
        .filter_map(|o| {
            let id = o["noticeId"].as_str()?.to_string();
            let title = o["title"].as_str()?.to_string();
            let link = o["uiLink"].as_str().map(|s| s.to_string()).unwrap_or_else(|| format!("https://sam.gov/opp/{id}/view"));
            Some(Found {
                id: format!("sam:{id}"),
                source: Source::Sam,
                kind: Kind::Contract,
                title: clip(&title, 140),
                link,
                summary: clip(
                    &format!(
                        "{}. {} NAICS {}.",
                        o["fullParentPathName"].as_str().unwrap_or(""),
                        o["type"].as_str().unwrap_or(""),
                        o["naicsCode"].as_str().unwrap_or("?")
                    ),
                    300,
                ),
                at: o["postedDate"].as_str().and_then(iso_day).unwrap_or(now),
                closes: o["responseDeadLine"].as_str().and_then(iso_day),
                pay: None,
                remote: false,
            })
        })
        .collect())
}

/// Items of an RSS or Atom feed.
pub fn feed_items(xml: &str, source: Source, kind: Kind, now: u64) -> Result<Vec<Found>, String> {
    let parsed = crate::feeds::parse(xml)?;
    Ok(parsed
        .items
        .into_iter()
        .filter_map(|i| {
            let link = crate::feeds::clean_link(&i.link)?;
            let summary = plain_text(&i.summary);
            Some(Found {
                id: format!("{}:{}", source.key(), if i.id.is_empty() { link.clone() } else { i.id.clone() }),
                source,
                kind,
                title: clip(&crate::feeds::text_of(&i.title), 140),
                pay: pay_in(&summary),
                remote: summary.to_lowercase().contains("remote"),
                summary: clip(&summary, 600),
                link,
                at: i.published.unwrap_or(now),
                closes: None,
            })
        })
        .collect())
}

/// A subreddit's feed. On r/forhire only `[Hiring]` posts count -- the rest
/// are people offering themselves, which is not an opportunity for you.
pub fn reddit(xml: &str, sub: &str, now: u64) -> Result<Vec<Found>, String> {
    let hiring_only = sub.eq_ignore_ascii_case("forhire") || sub.eq_ignore_ascii_case("slavelabour");
    let mut items = feed_items(xml, Source::Reddit, if hiring_only { Kind::Gig } else { Kind::Lead }, now)?;
    if hiring_only {
        items.retain(|f| {
            let t = f.title.to_lowercase();
            t.contains("[hiring]") || t.contains("[task]")
        });
    }
    Ok(items)
}

pub fn app_chart(json: &str, chart: &str, now: u64) -> Result<Vec<Found>, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("the App Store chart wasn't JSON: {e}"))?;
    let results = v["feed"]["results"].as_array().ok_or("the App Store chart had no results")?;
    let which = if chart.contains("paid") { "paid" } else { "free" };
    Ok(results
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let id = r["id"].as_str()?;
            let name = r["name"].as_str()?;
            Some(Found {
                id: format!("appstore:{which}:{id}"),
                source: Source::AppStore,
                kind: Kind::Niche,
                title: format!("{name} — number {} in top {which} apps", i + 1),
                link: r["url"].as_str().unwrap_or("").to_string(),
                summary: format!("By {}. Released {}.", r["artistName"].as_str().unwrap_or("?"), r["releaseDate"].as_str().unwrap_or("?")),
                at: now,
                closes: None,
                pay: None,
                remote: true,
            })
        })
        .collect())
}

pub fn github(json: &str, now: u64) -> Result<Vec<Found>, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("GitHub's answer wasn't JSON: {e}"))?;
    if let Some(m) = v["message"].as_str() {
        return Err(format!("GitHub said: {m}"));
    }
    let items = v["items"].as_array().ok_or("GitHub's answer had no items")?;
    Ok(items
        .iter()
        .filter_map(|r| {
            let name = r["full_name"].as_str()?;
            Some(Found {
                id: format!("github:{}", r["id"].as_i64().unwrap_or(0)),
                source: Source::Github,
                kind: Kind::Niche,
                title: format!("{name} — {} stars in its first week", r["stargazers_count"].as_i64().unwrap_or(0)),
                link: r["html_url"].as_str().unwrap_or("").to_string(),
                summary: clip(r["description"].as_str().unwrap_or(""), 300),
                at: r["created_at"].as_str().and_then(iso_day).unwrap_or(now),
                closes: None,
                pay: None,
                remote: true,
            })
        })
        .collect())
}

pub fn search_results(json: &str, now: u64) -> Result<Vec<Found>, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|_| "the search didn't answer with results -- is JSON turned on in SearXNG?".to_string())?;
    let results = v["results"].as_array().ok_or("the search answer had no results")?;
    Ok(results
        .iter()
        .filter_map(|r| {
            let link = crate::feeds::clean_link(r["url"].as_str()?)?;
            let summary = r["content"].as_str().unwrap_or("").to_string();
            Some(Found {
                id: format!("search:{link}"),
                source: Source::Search,
                kind: Kind::Lead,
                title: clip(r["title"].as_str().unwrap_or(&link), 140),
                pay: pay_in(&summary),
                remote: summary.to_lowercase().contains("remote"),
                summary: clip(&summary, 400),
                link,
                at: now,
                closes: None,
            })
        })
        .collect())
}

/// Job sites whose alert emails count, and what their alerts are.
const ALERT_SENDERS: &[(&str, Kind)] = &[
    ("upwork.com", Kind::Gig),
    ("fiverr.com", Kind::Gig),
    ("freelancer.com", Kind::Gig),
    ("peopleperhour.com", Kind::Gig),
    ("contra.com", Kind::Gig),
    ("toptal.com", Kind::Gig),
    ("linkedin.com", Kind::Job),
    ("indeed.com", Kind::Job),
    ("glassdoor.com", Kind::Job),
    ("wellfound.com", Kind::Job),
    ("weworkremotely.com", Kind::Job),
    ("remoteok.com", Kind::Job),
    ("otta.com", Kind::Job),
    ("grants.gov", Kind::Grant),
    ("sam.gov", Kind::Contract),
];

/// Job-alert emails already on this machine, from the sites above. Your own
/// mail, read here; nothing is fetched and nothing is answered.
pub fn alerts_in_mail(letters: &[crate::mailbook::Letter], since: u64) -> Vec<Found> {
    let mut out = Vec::new();
    for l in letters.iter().filter(|l| !l.mine && l.at >= since) {
        let domain = l.from.rsplit('@').next().unwrap_or("").to_lowercase();
        let Some((site, kind)) = ALERT_SENDERS.iter().find(|(s, _)| domain == *s || domain.ends_with(&format!(".{s}"))) else {
            continue;
        };
        // A receipt, a password reset or a login code is not an alert.
        let subj = l.subject.to_lowercase();
        if ["password", "verify", "verification", "receipt", "invoice", "security", "sign in", "login", "code"].iter().any(|w| subj.contains(w)) {
            continue;
        }
        let link = l
            .excerpt
            .split_whitespace()
            .find(|w| w.starts_with("https://"))
            .and_then(|w| crate::feeds::clean_link(w.trim_end_matches(['.', ',', ')', '>'])))
            .unwrap_or_default();
        out.push(Found {
            id: format!("mail:{}", l.id),
            source: Source::Mail,
            kind: *kind,
            title: clip(&l.subject, 140),
            link,
            summary: format!("{} ({site}): {}", l.from_name, clip(&l.excerpt, 400)),
            at: l.at,
            closes: None,
            pay: pay_in(&l.excerpt),
            remote: l.excerpt.to_lowercase().contains("remote"),
        });
    }
    out
}

// ---------------------------------------------------------------- dedupe

/// A title, lower-cased, letters and digits only, single-spaced.
fn title_key(t: &str) -> String {
    t.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn trigrams(s: &str) -> std::collections::HashSet<String> {
    let c: Vec<char> = s.chars().collect();
    if c.len() < 3 {
        return [s.to_string()].into_iter().collect();
    }
    c.windows(3).map(|w| w.iter().collect()).collect()
}

/// Trigram Jaccard of two normalised titles.
fn similarity(a: &str, b: &str) -> f32 {
    let (x, y) = (trigrams(a), trigrams(b));
    let inter = x.intersection(&y).count() as f32;
    let union = x.union(&y).count() as f32;
    if union == 0.0 { 0.0 } else { inter / union }
}

/// The key that makes two sightings of one thing the same: the clean link,
/// or the id when there is no link.
fn canonical_key(f: &Found) -> String {
    crate::feeds::clean_link(&f.link).unwrap_or_else(|| f.id.clone())
}

// ---------------------------------------------------------------- what you want

/// What you told Atlas to look for, kept as facts you stated.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Interests {
    pub want: Vec<String>,
    pub skills: Vec<String>,
    pub avoid: Vec<String>,
}

pub const FACT_WANT: &str = "opportunities-to-look-for";
pub const FACT_SKILLS: &str = "opportunity-skills";
pub const FACT_AVOID: &str = "opportunities-to-skip";

impl Interests {
    pub fn is_empty(&self) -> bool {
        self.want.is_empty() && self.skills.is_empty()
    }

    pub fn from_facts(book: &crate::facts::Book) -> Interests {
        let get = |name: &str| book.get(name).map(|f| list(&f.body)).unwrap_or_default();
        Interests { want: get(FACT_WANT), skills: get(FACT_SKILLS), avoid: get(FACT_AVOID) }
    }

    /// The facts, as you stated them. `which` is one of the three names.
    pub fn fact(which: &str, items: &[String], now: u64) -> crate::facts::Fact {
        let summary = match which {
            _ if items.is_empty() => format!("Nothing set for {}", which.replace('-', " ")),
            FACT_SKILLS => format!("Skills to match opportunities against: {}", items.join(", ")),
            FACT_AVOID => format!("Opportunities to skip: anything about {}", items.join(", ")),
            _ => format!("Opportunities worth looking for: {}", items.join(", ")),
        };
        let kind = if which == FACT_AVOID { crate::facts::Kind::Instruction } else { crate::facts::Kind::You };
        let mut f = crate::facts::Fact::new(which, &summary, &items.join(", "), kind, now);
        // A list, not a statement about one thing: no slot to supersede.
        f.subject = None;
        f.attribute = None;
        f.value = None;
        f
    }
}

fn has_term(text_words: &str, term: &str) -> bool {
    let t = title_key(term);
    !t.is_empty() && text_words.contains(&format!(" {t} "))
}

// ---------------------------------------------------------------- scoring

/// One found thing, weighed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ranked {
    pub found: Found,
    pub weighed: Opportunity,
    /// Higher is better. The grounded axes, Fit counted twice, before
    /// freshness.
    pub rank: f32,
    /// Why it's here, in a line or two.
    pub why: Vec<String>,
}

/// Content words of a title, for learning what you said no to.
fn title_content_words(t: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "the", "and", "for", "with", "you", "your", "are", "our", "from", "this", "that", "who", "into", "have",
        "will", "full", "time", "remote", "hiring", "job", "jobs", "senior", "engineer", "new", "app", "apps",
        "top", "free", "paid", "number", "in", "of", "a", "to", "an", "on", "at", "is", "or", "by",
    ];
    title_key(t)
        .split(' ')
        .filter(|w| w.len() > 2 && !STOP.contains(w) && !w.chars().all(|c| c.is_ascii_digit()))
        .map(|w| w.to_string())
        .collect()
}

/// Weigh one thing against what you want. `Err(reason)` when it's out.
pub fn weigh_found(f: &Found, you: &Interests, nope: &BTreeMap<String, u32>) -> Result<Ranked, String> {
    let words = format!(" {} ", title_key(&format!("{} {}", f.title, f.summary)));
    if let Some(a) = you.avoid.iter().find(|a| has_term(&words, a)) {
        return Err(format!("you said to skip anything about {a}"));
    }
    let learned: Vec<String> = title_content_words(&f.title).into_iter().filter(|w| nope.get(w).copied().unwrap_or(0) >= 2).collect();
    if learned.len() >= 2 {
        return Err(format!("like ones you said no to ({})", learned.join(", ")));
    }
    let wants: Vec<&String> = you.want.iter().filter(|w| has_term(&words, w)).collect();
    let skills: Vec<&String> = you.skills.iter().filter(|w| has_term(&words, w)).collect();
    if !you.is_empty() && wants.is_empty() && skills.is_empty() {
        return Err("nothing in it matches what you look for".into());
    }

    let mut o = Opportunity::new(&f.title, &format!("{} ({})", f.source.plain(), f.link));
    // Fit: what in it is yours.
    if you.is_empty() {
        o.note(Axis::Fit, Finding::Unknown("you haven't told me what to look for yet".into()));
    } else {
        let mut rests = Vec::new();
        if !skills.is_empty() {
            rests.push(format!("uses your {}", skills.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")));
        }
        if !wants.is_empty() {
            rests.push(format!("about {}, which you look for", wants.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")));
        }
        let n = (skills.len() * 2 + wants.len()).min(5) as u8;
        o.note(Axis::Fit, Finding::Scored { out_of_ten: 4 + n, rests_on: rests });
    }
    // Authenticity: who can post there, and whether it states its terms.
    let (base, why) = f.source.standing();
    let mut rests = vec![why.to_string()];
    let mut score = base;
    if let Some(p) = &f.pay {
        score = (score + 1).min(10);
        rests.push(format!("states its pay ({p})"));
    }
    o.note(Axis::Authenticity, Finding::Scored { out_of_ten: score, rests_on: rests });
    // Work: only what the listing says.
    let low = format!("{} {}", f.title, f.summary).to_lowercase();
    let work = [
        ("full-time", "full-time, by its own words"),
        ("full time", "full-time, by its own words"),
        ("part-time", "part-time, by its own words"),
        ("part time", "part-time, by its own words"),
        ("contract", "a contract, by its own words"),
        ("per hour", "paid by the hour"),
        ("/hour", "paid by the hour"),
        ("/hr", "paid by the hour"),
        ("one-off", "a one-off job"),
        ("fixed price", "fixed price"),
    ]
    .iter()
    .find(|(k, _)| low.contains(k));
    match (work, f.kind) {
        (Some((_, said)), _) => o.note(Axis::Work, Finding::Scored { out_of_ten: 6, rests_on: vec![said.to_string()] }),
        (None, Kind::Niche) => o.note(Axis::Work, Finding::Unknown("a niche, not a job: the work is whatever you'd build".into())),
        (None, _) => o.note(Axis::Work, Finding::Unknown("the listing doesn't say how much work".into())),
    }
    // Roadmap: is there somewhere to go next?
    match f.kind {
        Kind::Gig | Kind::Job | Kind::Grant | Kind::Contract if !f.link.is_empty() => o.note(
            Axis::Roadmap,
            Finding::Scored { out_of_ten: 6, rests_on: vec![format!("there's a listing to read and answer: {}", f.link)] },
        ),
        _ => o.note(Axis::Roadmap, Finding::Unknown("no first step is named; it's a signal to look into".into())),
    }
    // Money: never guessed.
    o.note(Axis::Money, crate::opportunity::money_is_not_auditable_yet());

    if let Verdict::SetAside { axis, why } = o.verdict() {
        return Err(format!("{axis:?} sank it: {why}"));
    }
    // `opportunity`'s own ordering: grounded scores only, Fit counted twice.
    let rank = o.rank(Axis::Fit).unwrap_or(0.0);
    let mut why = Vec::new();
    if let Some(Finding::Scored { rests_on, .. }) = o.finding(Axis::Fit) {
        why.push(rests_on.join("; "));
    }
    if let Some(Finding::Scored { rests_on, .. }) = o.finding(Axis::Authenticity) {
        why.push(rests_on.join("; "));
    }
    Ok(Ranked { found: f.clone(), weighed: o, rank, why })
}

/// How fresh, 0 to 1; `None` once it's past use.
pub fn freshness_of(f: &Found, now: u64) -> Option<f32> {
    if let Some(c) = f.closes {
        if c + 86_400 < now {
            return None;
        }
    }
    let (half, drop) = f.kind.shelf();
    let age_h = now.saturating_sub(f.at) / 3600;
    if f.closes.is_none() && age_h > drop {
        return None;
    }
    Some(0.5f32.powf(age_h as f32 / half as f32).max(0.05))
}

// ---------------------------------------------------------------- what's kept

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SourceStatus {
    pub last_read: u64,
    #[serde(default)]
    pub last_ok: u64,
    #[serde(default)]
    pub last_error: String,
    #[serde(default)]
    pub last_count: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HuntState {
    /// Canonical key -> when first seen.
    #[serde(default)]
    pub seen: BTreeMap<String, u64>,
    /// Recent normalised titles, for near-duplicates.
    #[serde(default)]
    pub titles: VecDeque<String>,
    #[serde(default)]
    pub shortlist: Vec<Ranked>,
    #[serde(default)]
    pub saved: Vec<Found>,
    /// Id -> why it's out, so it never comes back.
    #[serde(default)]
    pub rejected: BTreeMap<String, String>,
    /// Words from titles you said no to, and how often.
    #[serde(default)]
    pub nope: BTreeMap<String, u32>,
    #[serde(default)]
    pub sources: BTreeMap<String, SourceStatus>,
    /// The local day the request count is for.
    #[serde(default)]
    pub day: i64,
    #[serde(default)]
    pub requests_today: u32,
    #[serde(default)]
    pub last_run: u64,
    /// Asked once what to look for.
    #[serde(default)]
    pub asked: bool,
    /// Id -> when it was first put in a brief. A find is volunteered once
    /// (5 Oct 2026, Eric: "the opportunities are spamming me"): the brief is
    /// built for the morning, for every part-of-day hello and for every
    /// welcome back, and each one read out the same top finds again. Asking
    /// for them ("any opportunities?") still lists every one.
    #[serde(default)]
    pub briefed: BTreeMap<String, u64>,
}

pub const FILE: &str = "opportunities";

/// What a merge did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Merged {
    pub new: usize,
    pub duplicates: usize,
    pub rejected: usize,
}

impl HuntState {
    /// A new day starts the request count again.
    pub fn roll_day(&mut self, day: i64) {
        if self.day != day {
            self.day = day;
            self.requests_today = 0;
        }
    }

    pub fn left_today(&self, cfg: &HuntConfig) -> u32 {
        cfg.budget().saturating_sub(self.requests_today)
    }

    /// Take what a read found: dedupe, filter, weigh, keep.
    pub fn merge(&mut self, found: Vec<Found>, you: &Interests, now: u64) -> Merged {
        let mut m = Merged::default();
        for f in found {
            let key = canonical_key(&f);
            let tk = title_key(&f.title);
            if self.seen.contains_key(&key) || self.seen.contains_key(&f.id) || self.rejected.contains_key(&f.id) {
                m.duplicates += 1;
                continue;
            }
            // Cross-posts: the same title under a different link. Niches
            // from a chart are exempt -- "number 3 in top free" and "number
            // 4" differ by a digit and are different things.
            if tk.len() >= 12 && f.kind != Kind::Niche && self.titles.iter().any(|t| similarity(t, &tk) >= NEAR_DUPLICATE) {
                m.duplicates += 1;
                self.seen.insert(key, now);
                continue;
            }
            self.seen.insert(key, now);
            self.seen.insert(f.id.clone(), now);
            self.titles.push_back(tk);
            if freshness_of(&f, now).is_none() {
                m.rejected += 1;
                self.rejected.insert(f.id.clone(), "already stale when found".into());
                continue;
            }
            match weigh_found(&f, you, &self.nope) {
                Ok(r) => {
                    m.new += 1;
                    self.shortlist.push(r);
                }
                Err(why) => {
                    m.rejected += 1;
                    self.rejected.insert(f.id.clone(), why);
                }
            }
        }
        self.prune(now);
        m
    }

    /// Drop what's stale, and hold every list to its size.
    pub fn prune(&mut self, now: u64) {
        self.shortlist.retain(|r| freshness_of(&r.found, now).is_some());
        if self.shortlist.len() > KEEP_SHORTLIST {
            self.shortlist.sort_by(|a, b| b.rank.partial_cmp(&a.rank).unwrap_or(std::cmp::Ordering::Equal));
            self.shortlist.truncate(KEEP_SHORTLIST);
        }
        while self.titles.len() > KEEP_SEEN {
            self.titles.pop_front();
        }
        if self.seen.len() > KEEP_SEEN {
            let mut by_age: Vec<(u64, String)> = self.seen.iter().map(|(k, t)| (*t, k.clone())).collect();
            by_age.sort();
            for (_, k) in by_age.into_iter().take(self.seen.len() - KEEP_SEEN) {
                self.seen.remove(&k);
            }
        }
        while self.rejected.len() > KEEP_REJECTED {
            let first = self.rejected.keys().next().cloned();
            match first {
                Some(k) => {
                    self.rejected.remove(&k);
                }
                None => break,
            }
        }
    }

    /// The best `n` now, freshness counted.
    pub fn top(&self, n: usize, now: u64) -> Vec<&Ranked> {
        let mut v: Vec<(&Ranked, f32)> = self
            .shortlist
            .iter()
            .filter_map(|r| freshness_of(&r.found, now).map(|f| (r, r.rank * (0.5 + 0.5 * f))))
            .collect();
        v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then(b.0.found.at.cmp(&a.0.found.at)));
        v.into_iter().take(n).map(|(r, _)| r).collect()
    }

    /// The best `n` not yet volunteered in a brief, marked as volunteered.
    /// Marks older than a month are let go with the finds they were for.
    pub fn take_unbriefed(&mut self, n: usize, now: u64) -> Vec<Ranked> {
        let month = 30 * 86_400;
        self.briefed.retain(|_, at| now.saturating_sub(*at) < month);
        let briefed = &self.briefed;
        let mut v: Vec<(&Ranked, f32)> = self
            .shortlist
            .iter()
            .filter(|r| !briefed.contains_key(&r.found.id))
            .filter_map(|r| freshness_of(&r.found, now).map(|f| (r, r.rank * (0.5 + 0.5 * f))))
            .collect();
        v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then(b.0.found.at.cmp(&a.0.found.at)));
        let out: Vec<Ranked> = v.into_iter().take(n).map(|(r, _)| r.clone()).collect();
        for r in &out {
            self.briefed.insert(r.found.id.clone(), now);
        }
        out
    }

    pub fn find(&self, id: &str) -> Option<&Ranked> {
        self.shortlist.iter().find(|r| r.found.id == id)
    }

    /// "Not interested": out of the list for good, and its words noted so
    /// the next one like it is held back too (two words seen in two
    /// rejections).
    pub fn not_interested(&mut self, id: &str) -> Option<String> {
        let i = self.shortlist.iter().position(|r| r.found.id == id)?;
        let r = self.shortlist.remove(i);
        for w in title_content_words(&r.found.title) {
            *self.nope.entry(w).or_insert(0) += 1;
        }
        self.rejected.insert(r.found.id.clone(), "you said not interested".into());
        Some(r.found.title)
    }

    /// "Not interested in that kind": out of the list, and its words
    /// counted twice, so from now on anything sharing two of them is held
    /// back at once (`weigh`), where one "not interested" needs a second
    /// before it teaches anything.
    pub fn not_that_kind(&mut self, id: &str) -> Option<String> {
        let words = title_content_words(&self.find(id)?.found.title);
        let title = self.not_interested(id)?;
        for w in words {
            *self.nope.entry(w).or_insert(0) += 1;
        }
        Some(title)
    }

    /// "Save it": kept on the saved list, out of the running list.
    pub fn save(&mut self, id: &str) -> Option<Found> {
        let i = self.shortlist.iter().position(|r| r.found.id == id)?;
        let r = self.shortlist.remove(i);
        if !self.saved.iter().any(|s| s.id == r.found.id) {
            self.saved.push(r.found.clone());
        }
        Some(r.found)
    }

    pub fn note_source(&mut self, s: Source, took: &Took, now: u64) {
        let st = self.sources.entry(s.key().to_string()).or_default();
        st.last_read = now;
        match &took.error {
            Some(e) => st.last_error = e.clone(),
            None => {
                st.last_ok = now;
                st.last_error.clear();
                st.last_count = took.found.len();
            }
        }
        self.requests_today += took.requests;
    }
}

// ---------------------------------------------------------------- saying it

/// One line: what, where, why, and the weakest part.
pub fn line(n: usize, r: &Ranked) -> String {
    let mut s = format!("{n}. {} — {} {}", r.found.title, r.found.kind.plain(), from_where(r.found.source));
    if let Some(w) = r.why.first().filter(|w| !w.is_empty()) {
        s.push_str(&format!("; {w}"));
    }
    s
}

fn from_where(s: Source) -> String {
    format!("from {}", s.plain())
}

/// "Tell me more": everything it rests on.
pub fn more(r: &Ranked) -> String {
    let f = &r.found;
    let mut out = vec![format!("{} — a {} from {}.", f.title, f.kind.plain(), f.source.plain())];
    if !f.summary.is_empty() {
        out.push(f.summary.clone());
    }
    if let Some(p) = &f.pay {
        out.push(format!("Pay, as listed: {p}."));
    }
    if f.remote {
        out.push("Says remote.".into());
    }
    for l in &r.weighed.looks {
        let said = match &l.finding {
            Finding::Scored { out_of_ten, rests_on } => format!("{out_of_ten}/10 — {}", rests_on.join("; ")),
            Finding::Unknown(why) => format!("can't say — {why}"),
            Finding::Blocked(why) => format!("not yet — {why}"),
        };
        out.push(format!("{}: {said}", axis_word(l.axis)));
    }
    if !f.link.is_empty() {
        out.push(format!("It's at {}.", f.link));
    }
    out.push("I haven't applied, replied or signed up for anything. That's yours to do.".into());
    out.join("\n")
}

fn axis_word(a: Axis) -> &'static str {
    match a {
        Axis::Work => "The work",
        Axis::Authenticity => "Is it real",
        Axis::Roadmap => "First step",
        Axis::Money => "The money",
        Axis::Fit => "Fit",
    }
}

/// The question asked once, when nothing is known of what you want.
pub const ASK: &str = "What kinds of opportunities should I look for? Say \"look for opportunities in\" and a few \
                       words, and \"my skills are\" and yours -- or fill them in on the Opportunities page.";

// ---------------------------------------------------------------- by voice

/// What was said to the hunter.
#[derive(Debug, Clone, PartialEq)]
pub enum Said {
    /// "Any opportunities?"
    List,
    More(usize),
    NotInterested(usize),
    Save(usize),
    /// "Not interested in that kind": this one out, and anything like it
    /// held back from now on -- not just the next one that shares two words.
    NotThatKind(usize),
    LookFor(Vec<String>),
    Skills(Vec<String>),
    Avoid(Vec<String>),
    Start,
    Stop,
    Saved,
}

fn norm(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ',' || c == '\'' || c == '-' || c == '+' || c == '#' { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .filter(|w| w.trim_matches(',') != "atlas")
        .collect::<Vec<_>>()
        .join(" ")
        .trim_start_matches(',')
        .trim()
        .to_string()
}

fn split_items(rest: &str) -> Option<Vec<String>> {
    let v: Vec<String> = rest
        .replace(" and ", ",")
        .replace(" or ", ",")
        .split(',')
        .map(|s| s.trim().trim_start_matches("in ").trim_start_matches("about ").trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    (!v.is_empty()).then_some(v)
}

fn items_after(t: &str, leads: &[&str]) -> Option<Vec<String>> {
    for l in leads {
        if let Some(rest) = t.strip_prefix(l) {
            return split_items(rest);
        }
    }
    None
}

/// The words that make a sentence about opportunities rather than anything
/// else: "look for video editing *gigs*".
const KINDS: &[&str] = &[
    "gigs", "gig", "jobs", "job", "work", "contracts", "contract", "grants", "grant", "opportunities", "opportunity",
    "clients", "freelance work", "side hustles", "niches",
];

/// "look for video editing gigs" -> ["video editing"]. The sentence must
/// end in one of `KINDS`, so "look for my keys" is not this.
fn topic_then_kind(t: &str, leads: &[&str]) -> Option<Vec<String>> {
    for l in leads {
        let Some(rest) = t.strip_prefix(l) else { continue };
        for k in KINDS {
            if let Some(topic) = rest.strip_suffix(&format!(" {k}")) {
                let topic = topic.trim().trim_start_matches("some ").trim_start_matches("more ").trim();
                if topic.is_empty() || matches!(topic, "new" | "any" | "the" | "good" | "me" | "for me" | "more" | "some" | "other" | "my" | "your") {
                    return None;
                }
                return split_items(topic);
            }
        }
    }
    None
}

/// What stands for "the one we were just talking about" rather than a
/// place on the list: "not interested in that one", "save it". The daemon
/// reads it as the last one you asked more about.
pub const THAT_ONE: usize = usize::MAX;

/// "Crypto", but not "2", "the first", "that one" or "it".
fn a_subject(rest: &str) -> Option<Vec<String>> {
    let r = rest.trim();
    let words = r.split_whitespace().count();
    if r.is_empty() || words > 4 {
        return None;
    }
    let first = r.split_whitespace().next().unwrap_or("");
    const NOT: &[&str] = &[
        "that", "this", "it", "the", "one", "number", "any", "them", "those", "these", "first", "second", "third",
        "fourth", "fifth", "last", "no",
    ];
    if NOT.contains(&first) || r.trim_start_matches('#').chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    split_items(r)
}

/// Read a sentence meant for the hunter. `listed` is how many were on the
/// list just shown (0 when none is live), which is what lets "save 2" mean
/// the second one on it.
pub fn understand(said: &str, listed: usize) -> Option<Said> {
    let t = norm(said);
    let bare = t.trim_end_matches(['?', '.', '!']).trim();
    const LIST: &[&str] = &[
        "opportunities", "any opportunities", "any new opportunities", "show opportunities", "show me opportunities",
        "what opportunities", "what opportunities are there", "what opportunities have you found", "list opportunities",
        "show me the opportunities", "any gigs", "any new gigs", "find me gigs", "find me opportunities",
        "what have you found for me", "opportunities today", "my opportunities",
    ];
    if LIST.contains(&bare) {
        return Some(Said::List);
    }
    if ["saved opportunities", "my saved opportunities", "what opportunities did i save"].contains(&bare) {
        return Some(Said::Saved);
    }
    if ["start hunting", "start hunting for opportunities", "hunt for opportunities", "look for opportunities", "start looking for opportunities"].contains(&bare) {
        return Some(Said::Start);
    }
    if ["stop hunting", "stop hunting for opportunities", "stop looking for opportunities"].contains(&bare) {
        return Some(Said::Stop);
    }
    if let Some(v) = items_after(bare, &["look for opportunities in ", "look for opportunities about ", "look for opportunities like ", "hunt for opportunities in ", "opportunities i want are ", "find opportunities in "]) {
        return Some(Said::LookFor(v));
    }
    if let Some(v) = items_after(bare, &["my skills are ", "skills for opportunities are ", "match opportunities to "]) {
        return Some(Said::Skills(v));
    }
    if let Some(v) = items_after(bare, &["skip opportunities about ", "skip opportunities in ", "no opportunities about ", "never show me opportunities about "]) {
        return Some(Said::Avoid(v));
    }
    // "Look for video editing gigs", "find me rust contracts".
    if let Some(v) = topic_then_kind(bare, &["look for ", "find me ", "hunt for ", "keep an eye out for "]) {
        return Some(Said::LookFor(v));
    }
    // "Not interested in crypto gigs", "no crypto opportunities": said with
    // the kind, it's the hunter's even with no list showing.
    if let Some(v) = topic_then_kind(bare, &["not interested in ", "i'm not interested in ", "im not interested in ", "no ", "skip ", "no more "]) {
        return Some(Said::Avoid(v));
    }
    if listed == 0 {
        return None;
    }
    // A follow-up to the list just shown: "tell me more about 2", "not
    // interested in the first", "save 3". "Opportunity" or "one" may sit in
    // the middle ("save opportunity 2").
    let follow = bare.replace("opportunity ", "").replace("number ", "");
    let pick = |verbs: &[&str]| crate::workday::numbered_reply(&follow, verbs, listed).map(|(_, i)| i);
    // "that kind" first: "not interested in that kind" is more than "not
    // interested in that one".
    const THAT_KIND: &[&str] = &[
        "not interested in that kind", "not interested in that kind of thing", "not interested in those",
        "not interested in ones like that", "nothing like that", "no more like that", "none like that",
        "not that kind", "not that kind of thing", "no more of those", "fewer like that", "less like that",
    ];
    if THAT_KIND.contains(&bare) {
        return Some(Said::NotThatKind(if listed == 1 { 0 } else { THAT_ONE }));
    }
    if let Some(i) = pick(&["no more like", "nothing like", "none like", "fewer like"]) {
        return Some(Said::NotThatKind(i));
    }
    if let Some(i) = pick(&["tell me more about", "more about", "more on", "tell me about", "details on", "what about"]) {
        return Some(Said::More(i));
    }
    if let Some(i) = pick(&["not interested in", "no to", "drop", "not for me"]) {
        return Some(Said::NotInterested(i));
    }
    if let Some(i) = pick(&["save", "keep"]) {
        return Some(Said::Save(i));
    }
    // "That one" and "it": the one you last asked more about.
    let that = if listed == 1 { 0 } else { THAT_ONE };
    match bare {
        "not interested in that one" | "not interested in that" | "not interested in it" | "drop that one" | "drop it" => {
            return Some(Said::NotInterested(that))
        }
        "save that one" | "save that" | "keep that one" => return Some(Said::Save(that)),
        _ => {}
    }
    // With a list showing, "not interested in crypto" is about the list.
    for lead in ["not interested in ", "i'm not interested in ", "im not interested in "] {
        if let Some(v) = bare.strip_prefix(lead).and_then(a_subject) {
            return Some(Said::Avoid(v));
        }
    }
    // With a single one on the list, the number is optional.
    if listed == 1 {
        match bare {
            "tell me more" | "more" | "go on" => return Some(Said::More(0)),
            "not interested" | "not for me" => return Some(Said::NotInterested(0)),
            "save it" | "keep it" => return Some(Said::Save(0)),
            _ => {}
        }
    }
    None
}

// ---------- fit for one posting ----------
//
// The nine-repos report (career-ops, 1 Oct 2026): "how well do I fit this
// job?" over a whole posting. Only skills you've stated count (`Interests`
// from your facts) -- nothing is inferred, so the score can't flatter you.
// The posting is read as data, never as instructions: no model sees it here.

/// How a posting matches what you've told Atlas you can do.
#[derive(Debug, Clone, PartialEq)]
pub struct Fit {
    /// Your skills the posting names.
    pub matched: Vec<String>,
    /// What the posting asks for that you haven't said you have.
    pub missing: Vec<String>,
    /// Of ten: the share of what it asks for that you've stated.
    pub out_of_ten: u8,
}

/// Words a posting uses to say what it requires.
const ASKS_FOR: &[&str] = &[
    "require", "must", "experience with", "experience in", "proficien", "you have", "you'll have", "you will have",
    "qualification", "skills", "familiar", "knowledge of", "background in", "strong", "expert", "years of",
    "looking for", "nice to have", "bonus", "plus",
];

/// Capitalised words that aren't skills.
const NOT_SKILLS: &[&str] = &[
    "we", "you", "our", "the", "a", "an", "and", "or", "in", "with", "of", "for", "to", "experience", "strong",
    "bachelor", "bachelors", "master", "masters", "degree", "years", "year", "team", "teams", "requirements",
    "required", "qualifications", "skills", "must", "nice", "have", "bonus", "plus", "ability", "excellent",
    "remote", "us", "usa", "canada", "senior", "junior", "engineer", "developer", "role", "about", "what",
    "who", "why", "how", "responsibilities", "benefits", "salary", "equity", "knowledge", "familiarity",
    "proficiency", "proficient", "working", "work", "communication", "written", "verbal", "english", "i",
    "is", "are", "be", "at", "on", "as", "if", "it", "this", "that", "they", "their", "will", "can",
];

/// The named things a posting asks for: capitalised or symbol-bearing
/// words (React, AWS, C++, Node.js, Google Ads) on the lines that say
/// what's required, in the order first seen.
pub fn asked_for(posting: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in posting.lines().flat_map(|l| l.split(". ")) {
        let low = line.to_lowercase();
        if !ASKS_FOR.iter().any(|c| low.contains(c)) {
            continue;
        }
        let words: Vec<&str> = line.split(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '(' | ')' | '/' | ':')).filter(|w| !w.is_empty()).collect();
        let mut run: Vec<String> = Vec::new();
        let flush = |run: &mut Vec<String>, out: &mut Vec<String>| {
            if !run.is_empty() {
                let term = run.join(" ");
                if !out.iter().any(|o| o.eq_ignore_ascii_case(&term)) {
                    out.push(term);
                }
                run.clear();
            }
        };
        for (i, w) in words.iter().enumerate() {
            let w = w.trim_matches(|c: char| matches!(c, '.' | '!' | '?' | '"' | '\'' | '-' | '*' | '•'));
            let first = w.chars().next().unwrap_or(' ');
            let named = first.is_uppercase() || w.contains('+') || w.contains('#') || (w.contains('.') && w.len() > 2);
            let lead = i == 0 && w.chars().skip(1).all(|c| c.is_lowercase());
            if named && !lead && w.len() > 1 && !NOT_SKILLS.contains(&w.to_lowercase().as_str()) {
                run.push(w.to_string());
            } else {
                flush(&mut run, &mut out);
            }
        }
        flush(&mut run, &mut out);
    }
    out
}

/// How a posting fits what you've stated. `None` when you haven't stated any
/// skills or the posting names nothing it requires.
pub fn fit_to_posting(posting: &str, you: &Interests) -> Option<Fit> {
    if you.skills.is_empty() {
        return None;
    }
    let asked = asked_for(posting);
    let words = format!(" {} ", title_key(posting));
    let matched: Vec<String> = you.skills.iter().filter(|s| has_term(&words, s)).cloned().collect();
    let covered = |a: &String| you.skills.iter().any(|s| title_key(s) == title_key(a) || title_key(a).contains(&title_key(s)));
    let missing: Vec<String> = asked.iter().filter(|a| !covered(a)).cloned().collect();
    let total = matched.len() + missing.len();
    if total == 0 {
        return None;
    }
    let out_of_ten = ((matched.len() * 10 + total / 2) / total) as u8;
    Some(Fit { matched, missing, out_of_ten })
}

/// The fit, said: what matched, what's missing, and that only what you've
/// said counts.
pub fn fit_said(fit: &Fit) -> String {
    let mut out = format!("About {}/10 by what you've told me.", fit.out_of_ten);
    if !fit.matched.is_empty() {
        out.push_str(&format!(" It wants {}, which you have.", fit.matched.join(", ")));
    }
    if !fit.missing.is_empty() {
        let shown: Vec<&str> = fit.missing.iter().take(6).map(|s| s.as_str()).collect();
        let more = fit.missing.len().saturating_sub(shown.len());
        out.push_str(&format!(
            " Not in what you've told me: {}{}.",
            shown.join(", "),
            if more > 0 { format!(", and {more} more") } else { String::new() }
        ));
    }
    out.push_str(" If you have any of those, say \"my skills are\" and add them -- I only count what you've said.");
    out
}
