//! The free official APIs for your own accounts: what to ask, and what the
//! answer means.
//!
//! - **YouTube Data API v3** (an API key, free): your channel's subscriber
//!   and video counts and each video's views, likes and comments.
//!   `channels.list`, `playlistItems.list` and `videos.list` cost one unit
//!   each against 10,000 a day, so a daily refresh is three or four.
//! - **YouTube Analytics API** (your own Google sign-in, free): watch time,
//!   average view duration and percentage -- retention. The sign-in's
//!   refresh token **lasts seven days while your Google app is in
//!   "Testing"** (Google's OAuth policy), so Atlas says when it's about to
//!   lapse and what to do: publish the app (unverified is fine for your own
//!   use) or sign in again.
//! - **Instagram API with Instagram Login** (a Professional account and a
//!   long-lived token you make in Meta's app dashboard): followers, each
//!   post's likes and comments, and per-post insights (views, reach, saves,
//!   shares; reels' average watch time). Tokens last 60 days and are
//!   refreshed here after 30.
//! - **Bluesky**, the public AppView, no sign-in: followers and each post's
//!   likes, reposts, replies and quotes. Bluesky has no view counts.
//!
//! Every call goes through `Net`, so the reading of each answer is tested
//! against saved real-shaped replies without a network.

use super::snapshots::{AccountSnap, Metrics, Platform, PostSnap, Record};
use serde_json::Value;

/// A reply, with the two headers politeness needs.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Reply {
    pub status: u16,
    pub body: String,
    pub last_modified: Option<String>,
    /// Seconds the server asked us to wait.
    pub retry_after: Option<u64>,
}

/// The network, or a stand-in for it.
pub trait Net {
    fn get(&self, host: &str, path: &str, headers: &[(&str, &str)]) -> Result<Reply, String>;
    fn post_form(&self, host: &str, path: &str, form: &str) -> Result<Reply, String>;
    /// A JSON body (TikTok's `video.list` is a POST).
    fn post_json(&self, host: &str, path: &str, headers: &[(&str, &str)], body: &str) -> Result<Reply, String>;
}

/// The real one: TLS, a descriptive User-Agent, ten seconds.
pub struct Https;

/// Who Atlas says it is. Feed servers ask for a real description, and
/// Reddit refuses the default ones outright.
pub const USER_AGENT: &str = "PersonalAtlas/1.0 (a personal assistant reading public feeds for its owner; low volume)";

fn reply_of(r: crate::error::Result<(crate::http::Response, Vec<(String, String)>)>) -> Result<Reply, String> {
    let (resp, headers) = r.map_err(|e| e.to_string())?;
    let header = |k: &str| headers.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    Ok(Reply {
        status: resp.status,
        body: resp.body,
        last_modified: header("last-modified"),
        retry_after: header("retry-after").and_then(|v| v.trim().parse().ok()),
    })
}

impl Net for Https {
    fn get(&self, host: &str, path: &str, headers: &[(&str, &str)]) -> Result<Reply, String> {
        let mut h = vec![("User-Agent", USER_AGENT)];
        h.extend_from_slice(headers);
        reply_of(crate::http::https_call("GET", host, path, &h, None, std::time::Duration::from_secs(10)))
    }
    fn post_form(&self, host: &str, path: &str, form: &str) -> Result<Reply, String> {
        reply_of(crate::http::https_call(
            "POST",
            host,
            path,
            &[("User-Agent", USER_AGENT)],
            Some(("application/x-www-form-urlencoded", form)),
            std::time::Duration::from_secs(10),
        ))
    }
    fn post_json(&self, host: &str, path: &str, headers: &[(&str, &str)], body: &str) -> Result<Reply, String> {
        let mut h = vec![("User-Agent", USER_AGENT)];
        h.extend_from_slice(headers);
        reply_of(crate::http::https_call("POST", host, path, &h, Some(("application/json", body)), std::time::Duration::from_secs(10)))
    }
}

fn enc(s: &str) -> String {
    crate::research::urlencode(s)
}

fn json_of(r: Reply, what: &str) -> Result<Value, String> {
    let v: Value = serde_json::from_str(&r.body).unwrap_or(Value::Null);
    if !(200..300).contains(&r.status) {
        let why = v
            .pointer("/error/message")
            .or_else(|| v.pointer("/error_description"))
            .or_else(|| v.pointer("/message"))
            .and_then(|m| m.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("it answered {}", r.status));
        return Err(format!("{what}: {why}"));
    }
    if v.is_null() {
        return Err(format!("{what}: the answer wasn't JSON"));
    }
    Ok(v)
}

fn n(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

/// ISO 8601 duration ("PT1M3S", "P0D") to seconds.
pub fn iso_duration(s: &str) -> Option<f64> {
    let s = s.strip_prefix('P')?;
    let mut total = 0.0;
    let mut numb = String::new();
    let mut in_time = false;
    for c in s.chars() {
        match c {
            'T' => in_time = true,
            '0'..='9' | '.' => numb.push(c),
            u => {
                let v: f64 = numb.parse().ok()?;
                numb.clear();
                total += v * match (u, in_time) {
                    ('D', false) => 86_400.0,
                    ('W', false) => 604_800.0,
                    ('H', true) => 3600.0,
                    ('M', true) => 60.0,
                    ('S', true) => 1.0,
                    _ => return None,
                };
            }
        }
    }
    Some(total)
}

/// RFC 3339 to seconds ("2026-09-21T18:04:06.128Z", "...+00:00").
pub fn rfc3339(s: &str) -> Option<u64> {
    crate::feeds::parse_date(s)
}

// ---------------------------------------------------------------- YouTube

/// Your channel and its latest videos, from the Data API. Four units.
pub fn youtube_own(net: &dyn Net, key: &str, channel: &str, now: u64) -> Result<Vec<Record>, String> {
    let host = "www.googleapis.com";
    let who = channel.trim();
    let sel = if who.starts_with("UC") && !who.contains('@') { format!("id={}", enc(who)) } else { format!("forHandle={}", enc(&format!("@{}", who.trim_start_matches('@')))) };
    let ch = json_of(net.get(host, &format!("/youtube/v3/channels?part=statistics,contentDetails,snippet&{sel}&key={}", enc(key)), &[])?, "YouTube (your channel)")?;
    let item = ch.pointer("/items/0").ok_or_else(|| format!("YouTube has no channel {who}"))?;
    let stats = item.get("statistics").cloned().unwrap_or(Value::Null);
    let handle = item.pointer("/snippet/customUrl").and_then(|v| v.as_str()).unwrap_or(who).to_string();
    let hidden = stats.get("hiddenSubscriberCount").and_then(|v| v.as_bool()).unwrap_or(false);
    let mut out = vec![Record::Account(AccountSnap {
        platform: Platform::Youtube,
        handle: handle.clone(),
        day: super::social_day(now),
        taken: now,
        followers: if hidden { None } else { stats.get("subscriberCount").and_then(n) },
        following: None,
        posts: stats.get("videoCount").and_then(n),
        source: "the YouTube Data API".into(),
        m: Metrics { views: stats.get("viewCount").and_then(n), ..Default::default() },
    })];
    let uploads = item.pointer("/contentDetails/relatedPlaylists/uploads").and_then(|v| v.as_str()).ok_or("YouTube didn't say where your uploads are")?;
    let pl = json_of(net.get(host, &format!("/youtube/v3/playlistItems?part=contentDetails&maxResults=50&playlistId={}&key={}", enc(uploads), enc(key)), &[])?, "YouTube (your uploads)")?;
    let ids: Vec<String> = pl
        .get("items")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|i| i.pointer("/contentDetails/videoId").and_then(|v| v.as_str()).map(str::to_string))
        .collect();
    if ids.is_empty() {
        return Ok(out);
    }
    let vids = json_of(net.get(host, &format!("/youtube/v3/videos?part=statistics,contentDetails,snippet&id={}&key={}", ids.join(","), enc(key)), &[])?, "YouTube (your videos)")?;
    out.extend(youtube_videos(&vids, now, "the YouTube Data API"));
    Ok(out)
}

/// A `videos.list` answer, as snapshots.
pub fn youtube_videos(v: &Value, now: u64, source: &str) -> Vec<Record> {
    v.get("items")
        .and_then(|i| i.as_array())
        .into_iter()
        .flatten()
        .filter_map(|it| {
            let id = it.get("id")?.as_str()?.to_string();
            let s = it.get("statistics").cloned().unwrap_or(Value::Null);
            Some(Record::Post(PostSnap {
                platform: Platform::Youtube,
                url: format!("https://www.youtube.com/watch?v={id}"),
                id,
                day: super::social_day(now),
                taken: now,
                posted: it.pointer("/snippet/publishedAt").and_then(|x| x.as_str()).and_then(rfc3339),
                text: it.pointer("/snippet/title").and_then(|x| x.as_str()).unwrap_or("").chars().take(280).collect(),
                seconds: it.pointer("/contentDetails/duration").and_then(|x| x.as_str()).and_then(|d| iso_duration(d)),
                source: source.into(),
                m: Metrics {
                    views: s.get("viewCount").and_then(n),
                    likes: s.get("likeCount").and_then(n),
                    comments: s.get("commentCount").and_then(n),
                    ..Default::default()
                },
            }))
        })
        .collect()
}

/// What the YouTube Analytics API says per video over a range: retention
/// and watch time, merged into the Data API's snapshots for the same day.
pub fn youtube_analytics(net: &dyn Net, access_token: &str, start_day: i64, end_day: i64) -> Result<Vec<(String, Metrics)>, String> {
    let date = |d: i64| {
        let c = crate::civil::Civil::from_local(d * 86_400);
        format!("{:04}-{:02}-{:02}", c.year, c.month, c.day)
    };
    let path = format!(
        "/v2/reports?ids=channel%3D%3DMINE&startDate={}&endDate={}&metrics=views,estimatedMinutesWatched,averageViewDuration,averageViewPercentage,likes,comments,shares,subscribersGained&dimensions=video&sort=-views&maxResults=200",
        date(start_day),
        date(end_day)
    );
    let bearer = format!("Bearer {access_token}");
    let v = json_of(net.get("youtubeanalytics.googleapis.com", &path, &[("Authorization", &bearer)])?, "YouTube Analytics")?;
    Ok(analytics_rows(&v))
}

/// A report's rows by its column headers, so the order Google returns them
/// in doesn't matter.
pub fn analytics_rows(v: &Value) -> Vec<(String, Metrics)> {
    let heads: Vec<String> = v
        .get("columnHeaders")
        .and_then(|h| h.as_array())
        .into_iter()
        .flatten()
        .map(|h| h.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string())
        .collect();
    let at = |name: &str| heads.iter().position(|h| h == name);
    let mut out = Vec::new();
    for row in v.get("rows").and_then(|r| r.as_array()).into_iter().flatten() {
        let Some(r) = row.as_array() else { continue };
        let Some(id) = at("video").and_then(|i| r.get(i)).and_then(|x| x.as_str()) else { continue };
        let f = |name: &str| at(name).and_then(|i| r.get(i)).and_then(|x| x.as_f64());
        out.push((
            id.to_string(),
            Metrics {
                views: f("views").map(|x| x as u64),
                watch_minutes: f("estimatedMinutesWatched"),
                avg_view_secs: f("averageViewDuration"),
                avg_view_pct: f("averageViewPercentage"),
                likes: f("likes").map(|x| x as u64),
                comments: f("comments").map(|x| x as u64),
                shares: f("shares").map(|x| x as u64),
                followers_gained: f("subscribersGained").map(|x| x as i64),
                ..Default::default()
            },
        ));
    }
    out
}

/// Your Google sign-in for YouTube Analytics, as kept in the vault.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GoogleSignIn {
    pub client_id: String,
    pub client_secret: String,
    #[serde(default)]
    pub refresh_token: String,
    /// When the refresh token was issued: in Testing it lapses seven days on.
    #[serde(default)]
    pub obtained: u64,
}

/// A Testing app's sign-in lasts this long (Google OAuth policy).
pub const TESTING_LASTS: u64 = 7 * 86_400;

impl GoogleSignIn {
    /// Said when it's within a day of lapsing, or past it.
    pub fn lapsing(&self, now: u64, testing: bool) -> Option<String> {
        if !testing || self.refresh_token.is_empty() {
            return None;
        }
        let age = now.saturating_sub(self.obtained);
        if age >= TESTING_LASTS {
            Some("Your YouTube Analytics sign-in has lapsed: Google ends them after seven days while your app is in Testing. Sign in again from the Social page, or publish the app in Google's console (unverified is fine for your own use) and it stops lapsing.".into())
        } else if age + 86_400 >= TESTING_LASTS {
            Some("Your YouTube Analytics sign-in lapses within a day (Google's seven-day limit for apps in Testing). Sign in again from the Social page, or publish the app so it stops lapsing.".into())
        } else {
            None
        }
    }
}

pub const GOOGLE_SCOPE: &str = "https://www.googleapis.com/auth/yt-analytics.readonly https://www.googleapis.com/auth/youtube.readonly";

/// The consent page: your own browser, Google's own page, back to a port on
/// this machine (a "Desktop app" client, PKCE).
pub fn google_consent_url(client_id: &str, redirect: &str, state: &str, challenge: &str) -> String {
    format!(
        "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}&code_challenge={}&code_challenge_method=S256&access_type=offline&prompt=consent",
        enc(client_id),
        enc(redirect),
        enc(GOOGLE_SCOPE),
        enc(state),
        enc(challenge)
    )
}

/// PKCE's S256: base64url(sha256(verifier)), unpadded.
pub fn pkce_challenge(verifier: &str) -> String {
    use sha2::Digest;
    let d = sha2::Sha256::digest(verifier.as_bytes());
    crate::b64::encode(&d).replace('+', "-").replace('/', "_").trim_end_matches('=').to_string()
}

/// The code out of the request Google's redirect makes to this machine,
/// if its state matches the one sent.
pub fn code_from_redirect(request_line: &str, state: &str) -> Result<String, String> {
    let target = request_line.split_whitespace().nth(1).unwrap_or("");
    let query = target.split_once('?').map(|(_, q)| q).unwrap_or("");
    let mut code = None;
    let mut got_state = None;
    for kv in query.split('&') {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        let v = crate::hub::urldecode(v);
        match k {
            "code" => code = Some(v),
            "state" => got_state = Some(v),
            "error" => return Err(format!("Google said no: {v}")),
            _ => {}
        }
    }
    if got_state.as_deref() != Some(state) {
        return Err("that answer wasn't for this sign-in (its state didn't match), so I ignored it".into());
    }
    code.filter(|c| !c.is_empty()).ok_or_else(|| "Google came back without a code".into())
}

/// Swap the code for tokens. Gives back the refresh token.
pub fn google_exchange(net: &dyn Net, s: &GoogleSignIn, code: &str, redirect: &str, verifier: &str) -> Result<String, String> {
    let form = format!(
        "grant_type=authorization_code&code={}&client_id={}&client_secret={}&redirect_uri={}&code_verifier={}",
        enc(code),
        enc(&s.client_id),
        enc(&s.client_secret),
        enc(redirect),
        enc(verifier)
    );
    let v = json_of(net.post_form("oauth2.googleapis.com", "/token", &form)?, "Google sign-in")?;
    v.get("refresh_token").and_then(|t| t.as_str()).map(str::to_string).ok_or_else(|| "Google gave no refresh token (remove Atlas's access in your Google account and sign in again)".into())
}

/// A fresh access token from the refresh token. `invalid_grant` is the
/// seven-day lapse (or access taken away), said as that.
pub fn google_access(net: &dyn Net, s: &GoogleSignIn) -> Result<String, String> {
    let form = format!(
        "grant_type=refresh_token&refresh_token={}&client_id={}&client_secret={}",
        enc(&s.refresh_token),
        enc(&s.client_id),
        enc(&s.client_secret)
    );
    let r = net.post_form("oauth2.googleapis.com", "/token", &form)?;
    if r.body.contains("invalid_grant") {
        return Err("Google no longer accepts the YouTube Analytics sign-in -- it lapses after seven days while your app is in Testing, or access was removed. Sign in again from the Social page.".into());
    }
    let v = json_of(r, "Google sign-in")?;
    v.get("access_token").and_then(|t| t.as_str()).map(str::to_string).ok_or_else(|| "Google gave no access token".into())
}

// ---------------------------------------------------------------- Meta

/// The Graph API version asked for, Instagram and Facebook alike. Meta
/// keeps each version about two years: v25.0 (18 Feb 2026) is available
/// until 29 Jul 2028 (Meta's versions page, checked 29 Sep 2026). v21.0,
/// which this first asked for, ends on 21 Jan 2027.
pub const GRAPH_VERSION: &str = "v25.0";

const IG: &str = "graph.instagram.com";

/// Your account and your 25 latest posts, each with its insights. 27 calls.
pub fn instagram_own(net: &dyn Net, token: &str, now: u64) -> Result<Vec<Record>, String> {
    let t = enc(token);
    let me = json_of(net.get(IG, &format!("/{GRAPH_VERSION}/me?fields=user_id,username,followers_count,follows_count,media_count&access_token={t}"), &[])?, "Instagram (your account)")?;
    let handle = me.get("username").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let mut out = vec![Record::Account(AccountSnap {
        platform: Platform::Instagram,
        handle,
        day: super::social_day(now),
        taken: now,
        followers: me.get("followers_count").and_then(n),
        following: me.get("follows_count").and_then(n),
        posts: me.get("media_count").and_then(n),
        source: "the Instagram API".into(),
        m: Metrics::default(),
    })];
    let media = json_of(
        net.get(IG, &format!("/{GRAPH_VERSION}/me/media?fields=id,caption,media_type,media_product_type,timestamp,permalink,like_count,comments_count&limit=25&access_token={t}"), &[])?,
        "Instagram (your posts)",
    )?;
    for m in media.get("data").and_then(|d| d.as_array()).into_iter().flatten() {
        let Some(id) = m.get("id").and_then(|v| v.as_str()) else { continue };
        let reel = m.get("media_product_type").and_then(|v| v.as_str()) == Some("REELS");
        let metrics = if reel { "views,reach,saved,shares,ig_reels_avg_watch_time" } else { "views,reach,saved,shares" };
        // One post's insights failing (too new, or an old post type) costs
        // that post's insights, not the refresh.
        let ins = net.get(IG, &format!("/{GRAPH_VERSION}/{id}/insights?metric={metrics}&access_token={t}"), &[]).ok().and_then(|r| json_of(r, "insights").ok());
        let mut mm = Metrics { likes: m.get("like_count").and_then(n), comments: m.get("comments_count").and_then(n), ..Default::default() };
        if let Some(ins) = ins {
            mm.fill_from(&instagram_insights(&ins));
        }
        out.push(Record::Post(PostSnap {
            platform: Platform::Instagram,
            id: id.to_string(),
            day: super::social_day(now),
            taken: now,
            posted: m.get("timestamp").and_then(|v| v.as_str()).and_then(ig_time),
            text: m.get("caption").and_then(|v| v.as_str()).unwrap_or("").chars().take(280).collect(),
            url: m.get("permalink").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            seconds: None,
            source: "the Instagram API".into(),
            m: mm,
        }));
    }
    Ok(out)
}

/// Instagram's "2024-01-02T03:04:05+0000" (no colon in the offset).
fn ig_time(s: &str) -> Option<u64> {
    let fixed = if s.len() > 5 && (s.ends_with("+0000") || s.ends_with("-0000")) { format!("{}Z", &s[..s.len() - 5]) } else { s.to_string() };
    rfc3339(&fixed)
}

/// `/{media}/insights`'s `data: [{name, values: [{value}]}]`.
pub fn instagram_insights(v: &Value) -> Metrics {
    let mut m = Metrics::default();
    for d in v.get("data").and_then(|d| d.as_array()).into_iter().flatten() {
        let name = d.get("name").and_then(|x| x.as_str()).unwrap_or("");
        let val = d.pointer("/values/0/value").or_else(|| d.pointer("/total_value/value"));
        let Some(val) = val else { continue };
        match name {
            "views" => m.views = n(val),
            "reach" => m.reach = n(val),
            "saved" => m.saves = n(val),
            "shares" => m.shares = n(val),
            // Milliseconds.
            "ig_reels_avg_watch_time" => m.avg_view_secs = val.as_f64().map(|ms| ms / 1000.0),
            _ => {}
        }
    }
    m
}

/// A long-lived token's refresh (it must be at least a day old, and lasts
/// 60 days from each refresh).
pub fn instagram_refresh(net: &dyn Net, token: &str) -> Result<String, String> {
    let v = json_of(net.get(IG, &format!("/refresh_access_token?grant_type=ig_refresh_token&access_token={}", enc(token)), &[])?, "Instagram token refresh")?;
    v.get("access_token").and_then(|t| t.as_str()).map(str::to_string).ok_or_else(|| "Instagram gave no new token".into())
}

// ---------------------------------------------------------------- Bluesky

pub const BSKY: &str = "public.api.bsky.app";

/// Your profile and your 50 latest posts, from the public AppView.
pub fn bluesky_own(net: &dyn Net, handle: &str, now: u64) -> Result<Vec<Record>, String> {
    let h = enc(handle.trim().trim_start_matches('@'));
    let prof = json_of(net.get(BSKY, &format!("/xrpc/app.bsky.actor.getProfile?actor={h}"), &[])?, "Bluesky (your profile)")?;
    let mut out = vec![bluesky_profile(&prof, now)];
    let feed = json_of(net.get(BSKY, &format!("/xrpc/app.bsky.feed.getAuthorFeed?actor={h}&limit=50&filter=posts_no_replies"), &[])?, "Bluesky (your posts)")?;
    let me = prof.get("did").and_then(|d| d.as_str()).unwrap_or("");
    out.extend(bluesky_posts(&feed, me, now));
    Ok(out)
}

pub(super) fn bluesky_profile(p: &Value, now: u64) -> Record {
    Record::Account(AccountSnap {
        platform: Platform::Bluesky,
        handle: p.get("handle").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        day: super::social_day(now),
        taken: now,
        followers: p.get("followersCount").and_then(n),
        following: p.get("followsCount").and_then(n),
        posts: p.get("postsCount").and_then(n),
        source: "Bluesky's public API".into(),
        m: Metrics::default(),
    })
}

/// A `getAuthorFeed` answer's own posts (reposts of others left out).
pub(super) fn bluesky_posts(feed: &Value, author_did: &str, now: u64) -> Vec<Record> {
    feed.get("feed")
        .and_then(|f| f.as_array())
        .into_iter()
        .flatten()
        .filter(|item| item.get("reason").is_none())
        .filter_map(|item| {
            let p = item.get("post")?;
            let did = p.pointer("/author/did").and_then(|d| d.as_str()).unwrap_or("");
            if !author_did.is_empty() && did != author_did {
                return None;
            }
            let uri = p.get("uri")?.as_str()?;
            let rkey = uri.rsplit('/').next().unwrap_or(uri);
            let handle = p.pointer("/author/handle").and_then(|h| h.as_str()).unwrap_or("");
            Some(Record::Post(PostSnap {
                platform: Platform::Bluesky,
                id: uri.to_string(),
                day: super::social_day(now),
                taken: now,
                posted: p.pointer("/record/createdAt").and_then(|x| x.as_str()).and_then(rfc3339),
                text: p.pointer("/record/text").and_then(|x| x.as_str()).unwrap_or("").chars().take(280).collect(),
                url: format!("https://bsky.app/profile/{handle}/post/{rkey}"),
                seconds: None,
                source: "Bluesky's public API".into(),
                m: Metrics {
                    likes: p.get("likeCount").and_then(n),
                    reposts: p.get("repostCount").and_then(n),
                    comments: p.get("replyCount").and_then(n),
                    quotes: p.get("quoteCount").and_then(n),
                    ..Default::default()
                },
            }))
        })
        .collect()
}

// ---------------------------------------------------------------- Threads

const THREADS: &str = "graph.threads.net";

/// Your Threads account and its 25 latest posts, each with its insights,
/// through the Threads API (a long-lived token from your own Meta app in
/// development mode, you invited as its tester -- no App Review). Metric
/// names from Meta's Threads Insights reference (checked 29 Sep 2026):
/// per post `views, likes, replies, reposts, quotes, shares`; the account's
/// `followers_count` from `threads_insights`. 27 calls.
pub fn threads_own(net: &dyn Net, token: &str, now: u64) -> Result<Vec<Record>, String> {
    let t = enc(token);
    let me = json_of(net.get(THREADS, &format!("/v1.0/me?fields=id,username&access_token={t}"), &[])?, "Threads (your account)")?;
    let uid = me.get("id").and_then(|v| v.as_str()).unwrap_or("me").to_string();
    // The follower count is its own insight; a failure costs only that figure.
    let followers = net
        .get(THREADS, &format!("/v1.0/{}/threads_insights?metric=followers_count&access_token={t}", enc(&uid)), &[])
        .ok()
        .and_then(|r| json_of(r, "Threads followers").ok())
        .and_then(|v| v.pointer("/data/0/total_value/value").and_then(n));
    let mut out = vec![Record::Account(AccountSnap {
        platform: Platform::Threads,
        handle: me.get("username").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        day: super::social_day(now),
        taken: now,
        followers,
        following: None,
        posts: None,
        source: "the Threads API".into(),
        m: Metrics::default(),
    })];
    let list = json_of(net.get(THREADS, &format!("/v1.0/me/threads?fields=id,text,timestamp,permalink,media_type&limit=25&access_token={t}"), &[])?, "Threads (your posts)")?;
    for p in list.get("data").and_then(|d| d.as_array()).into_iter().flatten() {
        let Some(id) = p.get("id").and_then(|v| v.as_str()) else { continue };
        // A repost of someone else's has no insights of yours.
        if p.get("media_type").and_then(|v| v.as_str()) == Some("REPOST_FACADE") {
            continue;
        }
        let m = net
            .get(THREADS, &format!("/v1.0/{}/insights?metric=views,likes,replies,reposts,quotes,shares&access_token={t}", enc(id)), &[])
            .ok()
            .and_then(|r| json_of(r, "insights").ok())
            .map(|v| threads_insights(&v))
            .unwrap_or_default();
        out.push(Record::Post(PostSnap {
            platform: Platform::Threads,
            id: id.to_string(),
            day: super::social_day(now),
            taken: now,
            posted: p.get("timestamp").and_then(|v| v.as_str()).and_then(ig_time),
            text: p.get("text").and_then(|v| v.as_str()).unwrap_or("").chars().take(280).collect(),
            url: p.get("permalink").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            seconds: None,
            source: "the Threads API".into(),
            m,
        }));
    }
    Ok(out)
}

/// `/{media}/insights` on Threads: `data: [{name, values: [{value}]}]`.
pub fn threads_insights(v: &Value) -> Metrics {
    let mut m = Metrics::default();
    for d in v.get("data").and_then(|d| d.as_array()).into_iter().flatten() {
        let val = d.pointer("/values/0/value").or_else(|| d.pointer("/total_value/value")).and_then(n);
        match d.get("name").and_then(|x| x.as_str()).unwrap_or("") {
            "views" => m.views = val,
            "likes" => m.likes = val,
            "replies" => m.comments = val,
            "reposts" => m.reposts = val,
            "quotes" => m.quotes = val,
            "shares" => m.shares = val,
            _ => {}
        }
    }
    m
}

/// A Threads long-lived token's refresh (60 days from each).
pub fn threads_refresh(net: &dyn Net, token: &str) -> Result<String, String> {
    let v = json_of(net.get(THREADS, &format!("/refresh_access_token?grant_type=th_refresh_token&access_token={}", enc(token)), &[])?, "Threads token refresh")?;
    v.get("access_token").and_then(|t| t.as_str()).map(str::to_string).ok_or_else(|| "Threads gave no new token".into())
}

// ---------------------------------------------------------------- Facebook Page

const FB: &str = "graph.facebook.com";

/// Your Facebook Page and its 25 latest posts, through a Page access token.
/// Since 15 Nov 2025 Meta counts a Page's reach as `post_media_view` (it
/// replaced `post_impressions`, which is gone), so views here are that.
/// Reactions, comments and shares come from the post itself. 27 calls.
pub fn facebook_page_own(net: &dyn Net, page_token: &str, now: u64) -> Result<Vec<Record>, String> {
    let t = enc(page_token);
    let me = json_of(net.get(FB, &format!("/{GRAPH_VERSION}/me?fields=id,name,followers_count&access_token={t}"), &[])?, "Facebook (your Page)")?;
    let mut out = vec![Record::Account(AccountSnap {
        platform: Platform::Facebook,
        handle: me.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        day: super::social_day(now),
        taken: now,
        followers: me.get("followers_count").and_then(n),
        following: None,
        posts: None,
        source: "Facebook's Page API".into(),
        m: Metrics::default(),
    })];
    let posts = json_of(
        net.get(
            FB,
            &format!("/{GRAPH_VERSION}/me/posts?fields=id,message,created_time,permalink_url,shares,reactions.summary(total_count).limit(0),comments.summary(total_count).limit(0)&limit=25&access_token={t}"),
            &[],
        )?,
        "Facebook (your Page's posts)",
    )?;
    for p in posts.get("data").and_then(|d| d.as_array()).into_iter().flatten() {
        let Some(id) = p.get("id").and_then(|v| v.as_str()) else { continue };
        let views = net
            .get(FB, &format!("/{GRAPH_VERSION}/{}/insights?metric=post_media_view&access_token={t}", enc(id)), &[])
            .ok()
            .and_then(|r| json_of(r, "insights").ok())
            .and_then(|v| v.pointer("/data/0/values/0/value").and_then(n));
        out.push(Record::Post(PostSnap {
            platform: Platform::Facebook,
            id: id.to_string(),
            day: super::social_day(now),
            taken: now,
            posted: p.get("created_time").and_then(|v| v.as_str()).and_then(ig_time),
            text: p.get("message").and_then(|v| v.as_str()).unwrap_or("").chars().take(280).collect(),
            url: p.get("permalink_url").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            seconds: None,
            source: "Facebook's Page API".into(),
            m: Metrics {
                views,
                likes: p.pointer("/reactions/summary/total_count").and_then(n),
                comments: p.pointer("/comments/summary/total_count").and_then(n),
                // Meta leaves `shares` out when there are none, but an absent
                // field is kept as "not given", never written as zero.
                shares: p.pointer("/shares/count").and_then(n),
                ..Default::default()
            },
        }));
    }
    Ok(out)
}

// ---------------------------------------------------------------- TikTok

const TIKTOK: &str = "open.tiktokapis.com";

/// Your TikTok sign-in for the Display API, as kept in the vault: your own
/// app's key and secret (a Sandbox is enough -- no app review, you as its
/// target user) and the refresh token, which lasts a year and is replaced
/// at every use.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TikTokSignIn {
    pub client_key: String,
    pub client_secret: String,
    /// The redirect address registered in your app; the code comes back on it.
    pub redirect: String,
    #[serde(default)]
    pub refresh_token: String,
    /// The `state` sent with the consent page, until the code comes back.
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub obtained: u64,
}

pub const TIKTOK_SCOPE: &str = "user.info.basic,user.info.stats,video.list";

/// TikTok's consent page (Login Kit for Web, v2).
pub fn tiktok_consent_url(s: &TikTokSignIn) -> String {
    format!(
        "https://www.tiktok.com/v2/auth/authorize/?client_key={}&scope={}&response_type=code&redirect_uri={}&state={}",
        enc(&s.client_key),
        enc(TIKTOK_SCOPE),
        enc(&s.redirect),
        enc(&s.state)
    )
}

/// The code out of the address TikTok sent you to (pasted back), if its
/// state matches.
pub fn code_from_address(address: &str, state: &str) -> Result<String, String> {
    code_from_redirect(&format!("GET {} HTTP/1.1", address.trim()), state)
}

fn tiktok_tokens(v: &Value) -> Result<(String, String), String> {
    let access = v.get("access_token").and_then(|t| t.as_str()).ok_or("TikTok gave no access token")?;
    let refresh = v.get("refresh_token").and_then(|t| t.as_str()).ok_or("TikTok gave no refresh token")?;
    Ok((access.to_string(), refresh.to_string()))
}

/// Swap the code for tokens: (access token, refresh token).
pub fn tiktok_exchange(net: &dyn Net, s: &TikTokSignIn, code: &str) -> Result<(String, String), String> {
    let form = format!(
        "client_key={}&client_secret={}&code={}&grant_type=authorization_code&redirect_uri={}",
        enc(&s.client_key),
        enc(&s.client_secret),
        enc(code),
        enc(&s.redirect)
    );
    tiktok_tokens(&json_of(net.post_form(TIKTOK, "/v2/oauth/token/", &form)?, "TikTok sign-in")?)
}

/// A fresh access token (it lasts a day); TikTok may hand back a new
/// refresh token, which must replace the old one.
pub fn tiktok_access(net: &dyn Net, s: &TikTokSignIn) -> Result<(String, String), String> {
    let form = format!(
        "client_key={}&client_secret={}&grant_type=refresh_token&refresh_token={}",
        enc(&s.client_key),
        enc(&s.client_secret),
        enc(&s.refresh_token)
    );
    let r = net.post_form(TIKTOK, "/v2/oauth/token/", &form)?;
    if r.body.contains("invalid_grant") {
        return Err("TikTok no longer accepts the sign-in (a year has passed, or access was removed). Sign in again from the Social page.".into());
    }
    tiktok_tokens(&json_of(r, "TikTok sign-in")?)
}

/// TikTok answers `{"data": ..., "error": {"code": "ok"}}`; anything but
/// "ok" is its own refusal, said.
fn tiktok_json(r: Reply, what: &str) -> Result<Value, String> {
    let v = json_of(r, what)?;
    match v.pointer("/error/code").and_then(|c| c.as_str()) {
        None | Some("ok") => Ok(v),
        Some(code) => Err(format!("{what}: {}", v.pointer("/error/message").and_then(|m| m.as_str()).filter(|m| !m.is_empty()).unwrap_or(code))),
    }
}

/// Your account and 20 latest videos, from the Display API (`user.info`
/// and `video.list`). Views, likes, comments and shares per video; no
/// watch time or retention -- the Display API has none.
pub fn tiktok_own(net: &dyn Net, access_token: &str, now: u64) -> Result<Vec<Record>, String> {
    let bearer = format!("Bearer {access_token}");
    let h = [("Authorization", bearer.as_str())];
    let u = tiktok_json(net.get(TIKTOK, "/v2/user/info/?fields=open_id,display_name,follower_count,following_count,likes_count,video_count", &h)?, "TikTok (your account)")?;
    let user = u.pointer("/data/user").cloned().unwrap_or(Value::Null);
    let mut out = vec![Record::Account(AccountSnap {
        platform: Platform::Tiktok,
        handle: user.get("display_name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        day: super::social_day(now),
        taken: now,
        followers: user.get("follower_count").and_then(n),
        following: user.get("following_count").and_then(n),
        posts: user.get("video_count").and_then(n),
        source: "TikTok's Display API".into(),
        m: Metrics { likes: user.get("likes_count").and_then(n), ..Default::default() },
    })];
    let v = tiktok_json(
        net.post_json(TIKTOK, "/v2/video/list/?fields=id,title,video_description,create_time,duration,share_url,view_count,like_count,comment_count,share_count", &h, "{\"max_count\":20}")?,
        "TikTok (your videos)",
    )?;
    out.extend(tiktok_videos(&v, now));
    Ok(out)
}

/// A `video.list` answer, as snapshots.
pub fn tiktok_videos(v: &Value, now: u64) -> Vec<Record> {
    v.pointer("/data/videos")
        .and_then(|x| x.as_array())
        .into_iter()
        .flatten()
        .filter_map(|it| {
            let id = it.get("id").and_then(|x| x.as_str().map(str::to_string).or_else(|| x.as_u64().map(|n| n.to_string())))?;
            let title = it.get("title").and_then(|x| x.as_str()).filter(|t| !t.is_empty()).or_else(|| it.get("video_description").and_then(|x| x.as_str())).unwrap_or("");
            Some(Record::Post(PostSnap {
                platform: Platform::Tiktok,
                url: it.get("share_url").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                id,
                day: super::social_day(now),
                taken: now,
                posted: it.get("create_time").and_then(n),
                text: title.chars().take(280).collect(),
                seconds: it.get("duration").and_then(|x| x.as_f64()),
                source: "TikTok's Display API".into(),
                m: Metrics {
                    views: it.get("view_count").and_then(n),
                    likes: it.get("like_count").and_then(n),
                    comments: it.get("comment_count").and_then(n),
                    shares: it.get("share_count").and_then(n),
                    ..Default::default()
                },
            }))
        })
        .collect()
}
