//! Watching other people: channels, accounts, hashtags and topics you name,
//! read only where they're published in the open.
//!
//! | Source | Route | What it gives |
//! |---|---|---|
//! | YouTube channel | its public feed (`/feeds/videos.xml`), no key | the latest ~15 videos, each with views and likes |
//! | YouTube topic | Data API `search.list` (your key) | the week's most-viewed videos; **100 searches a day** for everyone, budgeted here |
//! | Mastodon hashtag | `/api/v1/timelines/tag/<tag>` on one server | public posts with favourites, boosts, replies -- what that server can see |
//! | Bluesky profile | public `getAuthorFeed` | posts with likes, reposts, replies, quotes (no views; search needs an account) |
//! | Hacker News | Algolia's HN API | stories with points and comments |
//! | Product Hunt | its Atom feed | the newest launches (no votes in the feed) |
//! | Google Trends | the daily trending RSS | today's searches with approximate traffic |
//! | Reddit | `/r/<sub>/new/.rss` | the newest posts -- **no scores or comment counts**: the API that has them needs approval |
//!
//! Live-checked from the build machine on 29 Sep 2026 (a datacenter address
//! behind a proxy): the YouTube feed, Bluesky, Hacker News, Product Hunt and
//! Google Trends answered 200; Bluesky search 403; mastodon.social 503 while
//! fosstodon.org answered 200; Reddit 429. Samples of each answer are in
//! `tests/fixtures/social`. Later that day, read through this module's own
//! reader (`https_call`, the parsers here; the ignored test
//! `live_public_sources_answer_through_atlass_own_reader`): every source
//! answered with counts -- mastodon.social too -- except Reddit, 403 from
//! that network, and Product Hunt, whose feed carries no votes.
//!
//! **Polite.** One read of a source per `scan_every_minutes` (never under
//! 30); `If-Modified-Since` where the server gives a date; a source that
//! fails backs off -- doubling to a day -- and a `Retry-After` is obeyed;
//! requests to one host are spaced two seconds apart. TikTok, Instagram and
//! X can't be added here at all (`onepage::WHY_NO_SCHEDULE`).

use super::apis::{Net, Reply};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const FILE: &str = "social_watch";
/// The most items kept from all sources together, newest first.
pub const MAX_SEEN: usize = 3000;
pub const MAX_TARGETS: usize = 100;
/// Google's own daily allowance of searches.
pub const YOUTUBE_SEARCH_CEILING: u32 = 100;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    /// `id` is the UC... id; empty until the handle has been looked up.
    YoutubeChannel { id: String, handle: String },
    YoutubeSearch { query: String },
    MastodonTag { instance: String, tag: String },
    BlueskyProfile { handle: String },
    /// An empty query is the front page.
    HackerNews { query: String },
    ProductHunt,
    GoogleTrends { geo: String },
    Reddit { sub: String },
}

impl Target {
    pub fn key(&self) -> String {
        match self {
            Target::YoutubeChannel { id, handle } => format!("yt:{}", if id.is_empty() { handle.to_lowercase() } else { id.clone() }),
            Target::YoutubeSearch { query } => format!("yts:{}", query.to_lowercase()),
            Target::MastodonTag { instance, tag } => format!("masto:{}#{}", instance.to_lowercase(), tag.to_lowercase()),
            Target::BlueskyProfile { handle } => format!("bsky:{}", handle.to_lowercase()),
            Target::HackerNews { query } => format!("hn:{}", query.to_lowercase()),
            Target::ProductHunt => "ph".into(),
            Target::GoogleTrends { geo } => format!("trends:{}", geo.to_uppercase()),
            Target::Reddit { sub } => format!("reddit:{}", sub.to_lowercase()),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Target::YoutubeChannel { handle, id } => format!("YouTube: {}", if handle.is_empty() { id } else { handle }),
            Target::YoutubeSearch { query } => format!("YouTube search: {query}"),
            Target::MastodonTag { instance, tag } => format!("Mastodon #{tag} (on {instance})"),
            Target::BlueskyProfile { handle } => format!("Bluesky: {handle}"),
            Target::HackerNews { query } if query.is_empty() => "Hacker News front page".into(),
            Target::HackerNews { query } => format!("Hacker News: {query}"),
            Target::ProductHunt => "Product Hunt".into(),
            Target::GoogleTrends { geo } => format!("Google trending searches ({geo})"),
            Target::Reddit { sub } => format!("r/{sub} (newest, no scores)"),
        }
    }

    fn host(&self) -> String {
        match self {
            Target::YoutubeChannel { .. } => "www.youtube.com".into(),
            Target::YoutubeSearch { .. } => "www.googleapis.com".into(),
            Target::MastodonTag { instance, .. } => instance.clone(),
            Target::BlueskyProfile { .. } => super::apis::BSKY.into(),
            Target::HackerNews { .. } => "hn.algolia.com".into(),
            Target::ProductHunt => "www.producthunt.com".into(),
            Target::GoogleTrends { .. } => "trends.google.com".into(),
            Target::Reddit { .. } => "www.reddit.com".into(),
        }
    }
}

/// A plain word, safe in a path.
fn word(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == '.').collect()
}

/// What you said, as the sources to watch. Refuses TikTok, Instagram and X
/// with the reason.
pub fn parse_target(said: &str, cfg: &super::SocialConfig) -> Result<Vec<Target>, String> {
    let low = said.to_ascii_lowercase();
    let rest = [
        "start watching ",
        "keep an eye on ",
        "watch the channel ",
        "watch the hashtag ",
        "watch the topic ",
        "watch the subreddit ",
        "follow the channel ",
        "follow the hashtag ",
        "follow the topic ",
        "follow the subreddit ",
        "follow the ",
        "follow ",
        "watch ",
    ]
        .iter()
        .find_map(|p| low.find(p).map(|i| said[i + p.len()..].trim().to_string()))
        .unwrap_or_else(|| said.trim().to_string());
    let r = rest.trim().trim_end_matches(['.', '?', '!']).to_string();
    let rl = r.to_ascii_lowercase();
    for (site, name) in [("tiktok", "TikTok"), ("instagram", "Instagram"), ("twitter", "X"), ("x.com", "X"), (" on x", "X")] {
        if rl.contains(site) || low.ends_with(site) {
            return Err(format!("I can't watch {name} on a schedule. {}", super::onepage::WHY_NO_SCHEDULE));
        }
    }
    if rl.ends_with(" on threads") || rl.contains("threads.net") || rl.contains("threads.com") {
        return Err("I can't watch other people on Threads: reading their posts needs Meta's approval, which isn't given for personal use. Your own Threads numbers I can read (the Social page).".into());
    }
    if rl.contains("hacker news") || rl == "hn" {
        return Ok(vec![Target::HackerNews { query: String::new() }]);
    }
    if rl.contains("product hunt") || rl.contains("producthunt") {
        return Ok(vec![Target::ProductHunt]);
    }
    if rl.contains("trend") && (rl.contains("google") || rl == "trends" || rl.contains("trending")) {
        return Ok(vec![Target::GoogleTrends { geo: cfg.trends_geo.trim().to_uppercase() }]);
    }
    if let Some(sub) = rl.strip_prefix("r/").or_else(|| rl.split("reddit.com/r/").nth(1)).or_else(|| rl.strip_prefix("the subreddit ")) {
        let sub = word(sub.split(['/', ' ']).next().unwrap_or(""));
        if !sub.is_empty() {
            return Ok(vec![Target::Reddit { sub }]);
        }
    }
    if let Some(tag) = rl.strip_prefix('#').or_else(|| rl.strip_prefix("hashtag ")).or_else(|| (low.contains("hashtag") && !rl.contains(' ')).then_some(rl.as_str())) {
        let tag = word(tag.split_whitespace().next().unwrap_or("").trim_start_matches('#'));
        if !tag.is_empty() {
            return Ok(vec![Target::MastodonTag { instance: word(&cfg.mastodon_instance), tag }]);
        }
    }
    if rl.contains("bsky") || rl.contains("bluesky") {
        let handle = r
            .split_whitespace()
            .map(|w| w.trim_start_matches('@').trim_end_matches(['.', ',']))
            .find(|w| w.contains('.') && !w.eq_ignore_ascii_case("bsky.app/profile"))
            .map(|w| w.rsplit('/').next().unwrap_or(w).to_string())
            .ok_or("Which Bluesky account? Give me its handle, like name.bsky.social.")?;
        return Ok(vec![Target::BlueskyProfile { handle: word(&handle) }]);
    }
    if rl.contains("youtube") || rl.starts_with('@') || rl.contains("/channel/uc") || rl.split_whitespace().any(|w| w.starts_with("uc") && w.len() == 24) {
        if let Some(i) = rl.find("/channel/") {
            let id: String = r[i + 9..].chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-').collect();
            return Ok(vec![Target::YoutubeChannel { id, handle: String::new() }]);
        }
        if let Some(w) = r.split_whitespace().find(|w| w.len() == 24 && w.starts_with("UC")) {
            return Ok(vec![Target::YoutubeChannel { id: word(w), handle: String::new() }]);
        }
        if let Some(h) = r.split(['/', ' ']).find(|w| w.starts_with('@')) {
            return Ok(vec![Target::YoutubeChannel { id: String::new(), handle: format!("@{}", word(h.trim_start_matches('@'))) }]);
        }
        return Err("Which YouTube channel? Give me its @handle or its address.".into());
    }
    // Anything else is a topic: Hacker News stories about it, and the
    // week's most-viewed YouTube videos on it when there's a key.
    let topic = rl.trim_start_matches("the topic ").trim_start_matches("topic ").trim().to_string();
    if topic.is_empty() {
        return Err("Watch what? A YouTube channel (@handle), a #hashtag, r/subreddit, a Bluesky handle, or a topic.".into());
    }
    Ok(vec![Target::HackerNews { query: topic.clone() }, Target::YoutubeSearch { query: topic }])
}

/// Is this sentence, whole, a watch-list request? For the parser's strict
/// shapes (`workday::read_first`): it starts "watch", "follow", "stop
/// watching"... and names a source by its mark -- an @handle, a #tag, an
/// r/subreddit -- or ends "on YouTube", "on Mastodon" and the like. "Watch
/// my hands" and "watch the disk" name none of those, so they stay with
/// whatever else reads them.
pub fn spoken_watch(said: &str) -> bool {
    let low = said.trim().trim_end_matches(['.', '!', '?']).to_lowercase();
    let starts = ["watch ", "follow ", "start watching ", "stop watching ", "unwatch ", "keep an eye on ", "stop following "];
    let Some(rest) = starts.iter().find_map(|p| low.strip_prefix(p)) else { return false };
    if ["the hashtag ", "the channel ", "the subreddit ", "hashtag "].iter().any(|p| rest.starts_with(p)) {
        return true;
    }
    let marked = rest.split_whitespace().any(|w| (w.starts_with('@') || w.starts_with('#') || w.starts_with("r/")) && w.len() > 1);
    let sites = ["youtube", "mastodon", "bluesky", "reddit", "hacker news", "product hunt", "tiktok", "instagram", "x", "twitter", "threads"];
    let on_site = sites.iter().any(|s| rest.ends_with(&format!(" on {s}")));
    marked || on_site
}

// ---------------------------------------------------------------- what's kept

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Watched {
    pub target: Target,
    #[serde(default)]
    pub next_due: u64,
    #[serde(default)]
    pub failures: u32,
    #[serde(default)]
    pub last_ok: Option<u64>,
    #[serde(default)]
    pub last_error: String,
    #[serde(default)]
    pub last_modified: Option<String>,
}

/// One thing seen: a video, a post, a story, a trending search.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Seen {
    /// Which watched source it came from (`Target::key`).
    pub from: String,
    pub id: String,
    pub who: String,
    pub title: String,
    pub url: String,
    pub at: Option<u64>,
    pub first_seen: u64,
    pub last_seen: u64,
    pub views: Option<u64>,
    pub likes: Option<u64>,
    pub comments: Option<u64>,
    pub reposts: Option<u64>,
    pub points: Option<u64>,
    /// Google's "500+", as written.
    pub traffic: Option<String>,
    /// Views at the read before this one, and when: how fast it's moving.
    pub views_before: Option<(u64, u64)>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Quota {
    /// The day in Pacific time -- when Google resets it.
    pub day: i64,
    pub used: u32,
}

impl Quota {
    /// Take one search if today's budget allows.
    pub fn take(&mut self, pacific_day: i64, budget: u32) -> bool {
        if self.day != pacific_day {
            self.day = pacific_day;
            self.used = 0;
        }
        let budget = budget.min(YOUTUBE_SEARCH_CEILING);
        if self.used >= budget {
            return false;
        }
        self.used += 1;
        true
    }

    pub fn left(&self, pacific_day: i64, budget: u32) -> u32 {
        let used = if self.day == pacific_day { self.used } else { 0 };
        budget.min(YOUTUBE_SEARCH_CEILING).saturating_sub(used)
    }
}

/// YouTube's quota day starts at midnight Pacific.
pub fn pacific_day(t: u64) -> i64 {
    let z = crate::tz::Zone::named("America/Los_Angeles").unwrap_or_else(|| crate::tz::Zone::fixed(-8 * 3600));
    z.to_local(t as i64).div_euclid(86_400)
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Watch {
    pub list: Vec<Watched>,
    pub seen: Vec<Seen>,
    pub quota: Quota,
    /// The last summary the model wrote, and when, for the Social page.
    pub summary: Option<(u64, String)>,
    /// When your own accounts were last refreshed.
    pub last_refresh: u64,
    /// When the Instagram token was kept or last refreshed (it lasts 60 days).
    pub instagram_token_at: u64,
    /// The same for Threads' token (also 60 days).
    pub threads_token_at: u64,
}

/// How a read went wrong: why, and how long the server asked us to wait.
#[derive(Debug, Clone, PartialEq)]
pub struct Failed {
    pub why: String,
    pub retry_after: Option<u64>,
}

/// What one read brought back.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Fetched {
    pub items: Vec<Seen>,
    pub last_modified: Option<String>,
    /// 304: nothing new since the last read.
    pub unchanged: bool,
    /// A channel whose handle was looked up to its id.
    pub resolved: Option<Target>,
}

impl Watch {
    /// Add, returning what was new; the same source twice is said, not doubled.
    pub fn add(&mut self, targets: Vec<Target>, now: u64) -> Result<Vec<String>, String> {
        let mut added = Vec::new();
        for t in targets {
            if self.list.iter().any(|w| w.target.key() == t.key()) {
                continue;
            }
            if self.list.len() >= MAX_TARGETS {
                return Err(format!("You're watching {MAX_TARGETS} things already -- stop watching one first."));
            }
            added.push(t.label());
            self.list.push(Watched { target: t, next_due: now, failures: 0, last_ok: None, last_error: String::new(), last_modified: None });
        }
        Ok(added)
    }

    /// Stop watching whatever matches `what`, and forget what it brought.
    pub fn remove(&mut self, what: &str) -> Vec<String> {
        // "the #rust hashtag on Mastodon" is "rust".
        let mut w = what.trim().trim_end_matches(['.', '!', '?']).to_lowercase();
        if let Some(i) = w.rfind(" on ") {
            w.truncate(i);
        }
        for p in ["the hashtag ", "the channel ", "the subreddit ", "the topic ", "the "] {
            if let Some(r) = w.strip_prefix(p) {
                w = r.to_string();
                break;
            }
        }
        let w = w.trim_end_matches(" hashtag").trim_end_matches(" channel").trim().trim_start_matches(['#', '@']).to_string();
        if w.is_empty() {
            return Vec::new();
        }
        // An exact name first ("@x" is not every source with an x in it);
        // only when nothing is named exactly, anything containing it.
        let exact = |x: &Watched| {
            let k = x.target.key();
            [":", "#", ":@"].iter().any(|sep| k.ends_with(&format!("{sep}{w}"))) || x.target.label().to_lowercase().ends_with(&format!(" {w}"))
        };
        let any_exact = self.list.iter().any(exact);
        let (gone, kept): (Vec<Watched>, Vec<Watched>) = self
            .list
            .drain(..)
            .partition(|x| if any_exact { exact(x) } else { x.target.key().contains(&w) || x.target.label().to_lowercase().contains(&w) });
        self.list = kept;
        let keys: Vec<String> = gone.iter().map(|g| g.target.key()).collect();
        self.seen.retain(|s| !keys.contains(&s.from));
        gone.iter().map(|g| g.target.label()).collect()
    }

    /// Sources due a read, oldest first.
    pub fn due(&self, now: u64) -> Vec<usize> {
        let mut v: Vec<usize> = (0..self.list.len()).filter(|&i| self.list[i].next_due <= now).collect();
        v.sort_by_key(|&i| self.list[i].next_due);
        v
    }

    /// A read came back.
    pub fn took(&mut self, key: &str, got: Fetched, now: u64, every_minutes: u64) {
        let Some(w) = self.list.iter_mut().find(|w| w.target.key() == key) else { return };
        if let Some(t) = got.resolved.clone() {
            w.target = t;
        }
        let key = w.target.key();
        w.failures = 0;
        w.last_ok = Some(now);
        w.last_error.clear();
        if got.last_modified.is_some() {
            w.last_modified = got.last_modified;
        }
        w.next_due = now + every_minutes.max(30) * 60;
        for mut item in got.items {
            item.from = key.clone();
            match self.seen.iter_mut().find(|s| s.from == item.from && s.id == item.id) {
                Some(old) => {
                    if let (Some(before), Some(_)) = (old.views, item.views) {
                        if old.last_seen < now {
                            item.views_before = Some((old.last_seen, before));
                        }
                    }
                    item.first_seen = old.first_seen;
                    item.last_seen = now;
                    *old = item;
                }
                None => {
                    item.first_seen = now;
                    item.last_seen = now;
                    self.seen.push(item);
                }
            }
        }
        // Newest kept, oldest dropped past the cap.
        if self.seen.len() > MAX_SEEN {
            self.seen.sort_by_key(|s| std::cmp::Reverse(s.at.unwrap_or(s.first_seen)));
            self.seen.truncate(MAX_SEEN);
        }
    }

    /// A read failed: back off, doubling to a day, and never sooner than the
    /// server asked.
    pub fn failed(&mut self, key: &str, f: &Failed, now: u64, every_minutes: u64) {
        let Some(w) = self.list.iter_mut().find(|w| w.target.key() == key) else { return };
        w.failures = w.failures.saturating_add(1);
        w.last_error = f.why.clone();
        let base = every_minutes.max(30) * 60;
        let backoff = base.saturating_mul(1u64 << w.failures.min(6)).min(86_400);
        w.next_due = now + backoff.max(f.retry_after.unwrap_or(0).min(86_400));
    }
}

// ---------------------------------------------------------------- reading one

/// Read one source. `yt_key` is the YouTube API key when there is one;
/// `may_search` says today's search budget has room (taken by the caller).
pub fn read_one(net: &dyn Net, t: &Target, last_modified: Option<&str>, yt_key: Option<&str>, may_search: bool, now: u64) -> Result<Fetched, Failed> {
    let fail = |why: String| Failed { why, retry_after: None };
    let enc = crate::research::urlencode;
    let mut resolved = None;
    let (host, path, conditional) = match t {
        Target::YoutubeChannel { id, handle } => {
            let id = if id.is_empty() {
                let found = resolve_channel(net, handle, yt_key).map_err(fail)?;
                resolved = Some(Target::YoutubeChannel { id: found.clone(), handle: handle.clone() });
                found
            } else {
                id.clone()
            };
            (t.host(), format!("/feeds/videos.xml?channel_id={}", enc(&id)), true)
        }
        Target::YoutubeSearch { query } => {
            let Some(key) = yt_key else { return Err(fail("YouTube search needs your API key (Social page)".into())) };
            if !may_search {
                return Err(Failed { why: "today's YouTube search budget is spent".into(), retry_after: Some(3600) });
            }
            let week_ago = now.saturating_sub(7 * 86_400);
            let c = crate::civil::Civil::from_local(week_ago as i64);
            let after = format!("{:04}-{:02}-{:02}T00:00:00Z", c.year, c.month, c.day);
            let r = check(net.get(&t.host(), &format!("/youtube/v3/search?part=snippet&type=video&order=viewCount&maxResults=10&publishedAfter={}&q={}&key={}", enc(&after), enc(query), enc(key)), &[]))?;
            let v: Value = serde_json::from_str(&r.body).map_err(|e| fail(format!("YouTube search didn't read: {e}")))?;
            let ids: Vec<&str> = v.get("items").and_then(|i| i.as_array()).into_iter().flatten().filter_map(|i| i.pointer("/id/videoId").and_then(|x| x.as_str())).collect();
            let mut items = Vec::new();
            if !ids.is_empty() {
                let r = check(net.get(&t.host(), &format!("/youtube/v3/videos?part=statistics,snippet&id={}&key={}", ids.join(","), enc(key)), &[]))?;
                let v: Value = serde_json::from_str(&r.body).map_err(|e| fail(format!("YouTube videos didn't read: {e}")))?;
                for it in v.get("items").and_then(|i| i.as_array()).into_iter().flatten() {
                    let s = it.get("statistics");
                    let num = |k: &str| s.and_then(|s| s.get(k)).and_then(|x| x.as_str()).and_then(|x| x.parse().ok());
                    let id = it.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    items.push(Seen {
                        url: format!("https://www.youtube.com/watch?v={id}"),
                        id,
                        who: it.pointer("/snippet/channelTitle").and_then(|x| x.as_str()).unwrap_or("").into(),
                        title: it.pointer("/snippet/title").and_then(|x| x.as_str()).unwrap_or("").into(),
                        at: it.pointer("/snippet/publishedAt").and_then(|x| x.as_str()).and_then(super::apis::rfc3339),
                        views: num("viewCount"),
                        likes: num("likeCount"),
                        comments: num("commentCount"),
                        ..Default::default()
                    });
                }
            }
            return Ok(Fetched { items, ..Default::default() });
        }
        Target::MastodonTag { instance, tag } => (instance.clone(), format!("/api/v1/timelines/tag/{}?limit=40", enc(tag)), false),
        Target::BlueskyProfile { handle } => (t.host(), format!("/xrpc/app.bsky.feed.getAuthorFeed?actor={}&limit=30&filter=posts_no_replies", enc(handle)), false),
        Target::HackerNews { query } if query.is_empty() => (t.host(), "/api/v1/search?tags=front_page&hitsPerPage=30".into(), false),
        Target::HackerNews { query } => {
            let since = now.saturating_sub(3 * 86_400);
            (t.host(), format!("/api/v1/search?query={}&tags=story&hitsPerPage=30&numericFilters=created_at_i%3E{since}", enc(query)), false)
        }
        Target::ProductHunt => (t.host(), "/feed".into(), true),
        Target::GoogleTrends { geo } => (t.host(), format!("/trending/rss?geo={}", enc(geo)), true),
        Target::Reddit { sub } => (t.host(), format!("/r/{}/new/.rss", enc(sub)), true),
    };
    let lm_header;
    let mut headers: Vec<(&str, &str)> = Vec::new();
    if conditional {
        if let Some(lm) = last_modified {
            lm_header = lm.to_string();
            headers.push(("If-Modified-Since", &lm_header));
        }
    }
    let r = net.get(&host, &path, &headers).map_err(fail)?;
    if r.status == 304 {
        return Ok(Fetched { unchanged: true, resolved, ..Default::default() });
    }
    let r = check(Ok(r))?;
    let items = match t {
        Target::YoutubeChannel { .. } => parse_youtube_feed(&r.body),
        Target::MastodonTag { .. } => parse_mastodon(&r.body),
        Target::BlueskyProfile { .. } => parse_bluesky(&r.body),
        Target::HackerNews { .. } => parse_hn(&r.body),
        Target::GoogleTrends { .. } => parse_trends(&r.body),
        Target::ProductHunt | Target::Reddit { .. } => parse_plain_feed(&r.body),
        Target::YoutubeSearch { .. } => Ok(Vec::new()),
    }
    .map_err(fail)?;
    Ok(Fetched { items, last_modified: r.last_modified, resolved, ..Default::default() })
}

fn check(r: Result<Reply, String>) -> Result<Reply, Failed> {
    let r = r.map_err(|why| Failed { why, retry_after: None })?;
    match r.status {
        200..=299 => Ok(r),
        429 | 503 => Err(Failed { why: format!("the site asked us to slow down ({})", r.status), retry_after: r.retry_after.or(Some(3600)) }),
        403 => Err(Failed { why: "the site refused (403) -- from some networks it refuses everyone".into(), retry_after: r.retry_after }),
        s => Err(Failed { why: format!("the site answered {s}"), retry_after: r.retry_after }),
    }
}

/// A channel's id from its @handle: the API when there's a key (one unit),
/// else the channel page's own canonical link, read once, signed out.
fn resolve_channel(net: &dyn Net, handle: &str, yt_key: Option<&str>) -> Result<String, String> {
    let h = handle.trim_start_matches('@');
    if let Some(key) = yt_key {
        let r = net.get("www.googleapis.com", &format!("/youtube/v3/channels?part=id&forHandle=%40{}&key={}", crate::research::urlencode(h), crate::research::urlencode(key)), &[])?;
        if let Some(id) = serde_json::from_str::<Value>(&r.body).ok().and_then(|v| v.pointer("/items/0/id").and_then(|x| x.as_str()).map(str::to_string)) {
            return Ok(id);
        }
    }
    let r = net.get("www.youtube.com", &format!("/@{}", crate::research::urlencode(h)), &[])?;
    channel_id_in_page(&r.body).ok_or_else(|| format!("I couldn't find the channel @{h}"))
}

/// `<link rel="canonical" href="https://www.youtube.com/channel/UC...">`.
pub fn channel_id_in_page(html: &str) -> Option<String> {
    let i = html.find("youtube.com/channel/UC")?;
    let id: String = html[i + 20..].chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-').collect();
    (id.len() == 24).then_some(id)
}

// ---------------------------------------------------------------- parsing

fn blocks<'a>(xml: &'a str, tag: &str) -> Vec<&'a str> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(a) = xml[i..].find(&open) {
        let s = i + a;
        let after = xml.as_bytes().get(s + open.len()).copied().unwrap_or(b'>');
        if !(after == b'>' || after == b' ' || after == b'\n' || after == b'\t') {
            i = s + open.len();
            continue;
        }
        let Some(e) = xml[s..].find(&close) else { break };
        out.push(&xml[s..s + e]);
        i = s + e + close.len();
    }
    out
}

fn inner(block: &str, tag: &str) -> Option<String> {
    let b = blocks(block, tag).into_iter().next()?;
    let body = &b[b.find('>')? + 1..];
    let body = body.trim();
    let body = body.strip_prefix("<![CDATA[").and_then(|x| x.strip_suffix("]]>")).unwrap_or(body);
    Some(crate::feeds::text_of(body))
}

fn attr_in(block: &str, tag: &str, name: &str) -> Option<String> {
    let i = block.find(&format!("<{tag}"))?;
    let end = block[i..].find('>')? + i;
    let t = &block[i..end];
    let k = t.find(&format!(" {name}=\""))? + name.len() + 3;
    let v = &t[k..];
    Some(v[..v.find('"')?].to_string())
}

/// A YouTube channel feed: each video with its public view and like counts.
pub fn parse_youtube_feed(xml: &str) -> Result<Vec<Seen>, String> {
    if !xml.contains("<feed") {
        return Err("that isn't a YouTube feed".into());
    }
    let channel = inner(xml.split("<entry").next().unwrap_or(""), "title").unwrap_or_default();
    Ok(blocks(xml, "entry")
        .into_iter()
        .filter_map(|e| {
            let id = inner(e, "yt:videoId")?;
            Some(Seen {
                url: format!("https://www.youtube.com/watch?v={id}"),
                id,
                who: inner(e, "name").unwrap_or_else(|| channel.clone()),
                title: inner(e, "title").unwrap_or_default(),
                at: inner(e, "published").and_then(|p| crate::feeds::parse_date(&p)),
                views: attr_in(e, "media:statistics", "views").and_then(|v| v.parse().ok()),
                // Since dislikes went private, the star rating's count is the
                // like count.
                likes: attr_in(e, "media:starRating", "count").and_then(|v| v.parse().ok()),
                ..Default::default()
            })
        })
        .collect())
}

/// Google's daily trending searches, with their approximate traffic.
pub fn parse_trends(xml: &str) -> Result<Vec<Seen>, String> {
    if !xml.contains("<rss") {
        return Err("that isn't Google's trends feed".into());
    }
    Ok(blocks(xml, "item")
        .into_iter()
        .filter_map(|it| {
            let title = inner(it, "title")?;
            Some(Seen {
                id: title.to_lowercase(),
                who: inner(it, "ht:news_item_source").unwrap_or_default(),
                url: inner(it, "ht:news_item_url").unwrap_or_default(),
                at: inner(it, "pubDate").and_then(|d| crate::feeds::parse_date(&d)),
                traffic: inner(it, "ht:approx_traffic"),
                title,
                ..Default::default()
            })
        })
        .collect())
}

/// Product Hunt and Reddit: an ordinary feed, no counts in it.
pub fn parse_plain_feed(xml: &str) -> Result<Vec<Seen>, String> {
    let p = crate::feeds::parse(xml)?;
    Ok(p.items
        .into_iter()
        .map(|i| Seen { id: i.id, title: i.title, url: i.link, at: i.published, ..Default::default() })
        .collect())
}

pub fn parse_hn(json: &str) -> Result<Vec<Seen>, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| format!("Hacker News didn't read: {e}"))?;
    Ok(v.get("hits")
        .and_then(|h| h.as_array())
        .into_iter()
        .flatten()
        .filter_map(|h| {
            let id = h.get("objectID")?.as_str()?.to_string();
            Some(Seen {
                url: h.get("url").and_then(|u| u.as_str()).map(str::to_string).unwrap_or_else(|| format!("https://news.ycombinator.com/item?id={id}")),
                id,
                who: h.get("author").and_then(|a| a.as_str()).unwrap_or("").into(),
                title: h.get("title").and_then(|t| t.as_str()).unwrap_or("").into(),
                at: h.get("created_at_i").and_then(|t| t.as_u64()),
                points: h.get("points").and_then(|p| p.as_u64()),
                comments: h.get("num_comments").and_then(|c| c.as_u64()),
                ..Default::default()
            })
        })
        .collect())
}

pub fn parse_mastodon(json: &str) -> Result<Vec<Seen>, String> {
    let v: Value = serde_json::from_str(json).map_err(|_| "that Mastodon server didn't answer with posts (some refuse visitors who aren't signed in)".to_string())?;
    let arr = v.as_array().ok_or("that Mastodon server didn't answer with posts")?;
    Ok(arr
        .iter()
        .filter_map(|s| {
            Some(Seen {
                id: s.get("id")?.as_str()?.to_string(),
                who: s.pointer("/account/acct").and_then(|a| a.as_str()).unwrap_or("").into(),
                title: crate::feeds::text_of(s.get("content").and_then(|c| c.as_str()).unwrap_or("")).chars().take(200).collect(),
                url: s.get("url").and_then(|u| u.as_str()).unwrap_or("").into(),
                at: s.get("created_at").and_then(|c| c.as_str()).and_then(crate::feeds::parse_date),
                likes: s.get("favourites_count").and_then(|x| x.as_u64()),
                reposts: s.get("reblogs_count").and_then(|x| x.as_u64()),
                comments: s.get("replies_count").and_then(|x| x.as_u64()),
                ..Default::default()
            })
        })
        .collect())
}

pub fn parse_bluesky(json: &str) -> Result<Vec<Seen>, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| format!("Bluesky didn't read: {e}"))?;
    Ok(super::apis::bluesky_posts(&v, "", 0)
        .into_iter()
        .filter_map(|r| match r {
            super::snapshots::Record::Post(p) => Some(Seen {
                who: p.url.split("/profile/").nth(1).and_then(|x| x.split('/').next()).unwrap_or("").into(),
                id: p.id,
                title: p.text.chars().take(200).collect(),
                url: p.url,
                at: p.posted,
                likes: p.m.likes,
                reposts: p.m.reposts,
                comments: p.m.comments,
                ..Default::default()
            }),
            _ => None,
        })
        .collect())
}
