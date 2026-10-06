//! Your social accounts, and what's working for the people you watch.
//!
//! Two halves, one rule. **Your own accounts**: the numbers each platform
//! will give you for free -- its "download your data" export, or its own API
//! where that takes only a key or your own sign-in -- kept here as daily
//! snapshots, because the platforms keep 28 to 90 days and Atlas keeps
//! everything it has seen. **Other people's**: only what they publish in the
//! open (feeds, public timelines, public APIs), read politely and cached.
//! The rule for both: **a number is only ever one that came out of the
//! data**. Where a platform gives nothing, the answer names the platform and
//! says what's missing; it never fills the gap with an estimate.
//!
//! What is not possible, said plainly (research of 28 Sep 2026, see
//! `THE_WALLS`): X has no free API and its export has no impressions;
//! TikTok gives no watch time or retention to anyone outside TikTok; LinkedIn
//! analytics come only as the export file; a personal Facebook profile has
//! no insights (a Page does); Reddit's API needs approval
//! nobody gets, so its feeds come without scores; Bluesky search needs an
//! account; and there is no legitimate free way to follow other people's
//! TikTok, Instagram or X on a schedule -- Atlas opens one page when you
//! ask, in its own browser, and never crawls them (`onepage`).
//!
//! Pieces: `snapshots` (the append-only record), `exports` (the export
//! files), `xlsx` (LinkedIn's spreadsheet), `apis` (the free official
//! APIs), `analysis` (what the numbers say), `watchlist` (scanning others),
//! `onepage` (the one page you ask for), `page` (the hub's Social page),
//! and `glue` (the daemon side: what you say, the tick, the brief).

pub mod analysis;
pub mod apis;
pub mod exports;
mod glue;
pub mod onepage;
pub mod posting;
pub use posting::post_with_app_password;
pub mod page;
pub mod snapshots;
pub mod watchlist;
pub mod xlsx;

/// "Watch @name on YouTube" as a whole sentence (the parser's strict shapes).
pub use watchlist::spoken_watch;

use serde::{Deserialize, Serialize};

/// Where the switches live (`workday.social` in tools.yaml).
///
/// Everything that fetches **on a schedule** is off until you turn it on:
/// `own_refresh` (your accounts' numbers, once a day) and `scan` (the people
/// and topics you watch). Asking still works with both off -- "refresh my
/// numbers", "what's trending" -- because an ask is you choosing to go out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SocialConfig {
    /// Answering about your accounts at all (from what's been imported).
    pub enabled: bool,
    /// Fetch your own accounts' numbers from their official APIs once a day.
    pub own_refresh: bool,
    /// Scan your watch list on a schedule.
    pub scan: bool,
    /// Hours between refreshes of your own accounts.
    pub refresh_hours: u64,
    /// Minutes between reads of any one watched source (never under 30).
    pub scan_every_minutes: u64,
    /// Your YouTube channel: its id (UC...) or its @handle.
    pub youtube_channel: String,
    /// Your Bluesky handle (e.g. jordan.bsky.social).
    pub bluesky_handle: String,
    /// Read your Instagram through the Instagram API (needs a Professional
    /// account and a token in the vault).
    pub instagram: bool,
    /// Read your Threads through the Threads API (a token in the vault).
    pub threads: bool,
    /// Read your Facebook Page through a Page token in the vault.
    pub facebook_page: bool,
    /// Read your TikTok through the Display API (your own app's Sandbox,
    /// signed in once from the Social page).
    pub tiktok: bool,
    /// Read retention and watch time from the YouTube Analytics API (needs
    /// "connect YouTube analytics" once).
    pub youtube_analytics: bool,
    /// Your Google app is still in "Testing", so its sign-in lasts seven
    /// days. Set false once you've published it (unverified is fine).
    pub google_app_in_testing: bool,
    /// YouTube searches a day for watched topics. Google allows 100; this
    /// is the share Atlas may spend.
    pub youtube_searches_per_day: u32,
    /// The Mastodon server whose public hashtag timelines are read.
    pub mastodon_instance: String,
    /// The country for Google's daily trending searches.
    pub trends_geo: String,
    /// A line in the morning brief when there's something worth saying.
    pub in_brief: bool,
}

impl Default for SocialConfig {
    fn default() -> Self {
        SocialConfig {
            enabled: true,
            own_refresh: false,
            scan: false,
            refresh_hours: 24,
            scan_every_minutes: 60,
            youtube_channel: String::new(),
            bluesky_handle: String::new(),
            instagram: false,
            threads: false,
            facebook_page: false,
            tiktok: false,
            youtube_analytics: false,
            google_app_in_testing: true,
            youtube_searches_per_day: 20,
            mastodon_instance: "mastodon.social".into(),
            trends_geo: "US".into(),
            in_brief: true,
        }
    }
}

/// The vault entries this reads. Keys and tokens live there and nowhere
/// else; the Social page's form puts them in.
pub const VAULT_YOUTUBE_KEY: &str = "youtube api key";
pub const VAULT_YOUTUBE_OAUTH: &str = "youtube analytics sign-in";
pub const VAULT_INSTAGRAM: &str = "instagram token";
pub const VAULT_THREADS: &str = "threads token";
pub const VAULT_FACEBOOK: &str = "facebook page token";
pub const VAULT_TIKTOK: &str = "tiktok sign-in";
/// The app password Bluesky posts with (`posting`), for the handle in
/// `bluesky_handle`.
pub const VAULT_BLUESKY: &str = "bluesky app password";

/// What each route needs from you, step by step, in the words the Social
/// page shows. Everything else works without any of it: imports need
/// nothing, and a route with no key is named as not set up, never an error.
pub const SETUP: &[(&str, &[&str])] = &[
    ("YouTube: views, likes and subscribers", &[
        "Go to console.cloud.google.com and sign in with the Google account that owns your channel.",
        "Make a project (any name), then open APIs & Services, Library, and turn on \"YouTube Data API v3\".",
        "Open Credentials, Create credentials, API key. Copy it.",
        "Paste it below as the YouTube API key, and put your channel's @handle under Your accounts.",
        "It's free: a daily refresh uses about four of the 10,000 units Google gives each day.",
    ]),
    ("YouTube: retention and watch time", &[
        "In the same Google project, turn on \"YouTube Analytics API\".",
        "Open OAuth consent screen: choose External, add yourself as a test user.",
        "Open Credentials, Create credentials, OAuth client ID, type \"Desktop app\". Copy its ID and secret.",
        "Paste both below and press Sign in; say yes on Google's page.",
        "While the app says \"Testing\", Google ends the sign-in after seven days. To stop that, press \"Publish app\" on the consent screen -- it stays unverified, which is fine for your own use; you click past one warning when you sign in.",
    ]),
    ("Instagram", &[
        "Your Instagram must be a Professional account (Creator or Business) -- switch in the app's settings; it's free.",
        "At developers.facebook.com make an app, add the \"Instagram\" product, and choose \"API setup with Instagram login\".",
        "Add your Instagram account there and generate its token. Copy it.",
        "Paste it below as the Instagram token and turn Instagram on under Your accounts. I renew it every 30 days; it lasts 60.",
    ]),
    ("Threads", &[
        "In the same kind of Meta app, add the \"Threads API\" product; the app can stay in development mode.",
        "Under Roles, add your Threads account as a Threads tester and accept the invite in Threads (Settings, Account, Website permissions).",
        "Generate a long-lived token with threads_basic and threads_manage_insights. Paste it below as the Threads token.",
    ]),
    ("Facebook Page", &[
        "Only a Page has insights; a personal profile has none.",
        "In the Graph API Explorer (developers.facebook.com/tools/explorer) pick your app, ask for pages_read_engagement and read_insights, and get a User token.",
        "Choose your Page in the same box to get its Page token, then extend it in the Access Token Debugger so it doesn't expire. Paste it below as the Facebook Page token.",
    ]),
    ("TikTok", &[
        "At developers.tiktok.com make an app and add a Sandbox -- no review needed. Add Login Kit and the Display API (scopes user.info.basic, user.info.stats, video.list).",
        "In the Sandbox, add your own TikTok account as a target user, and register a redirect address (any https address you own; the page doesn't need to load).",
        "Paste the client key, client secret and that redirect address below and press Sign in. After you say yes, TikTok sends you to the redirect address: copy that whole address from the browser's address bar and paste it in the second box.",
        "TikTok gives views, likes, comments and shares per video -- never watch time or retention.",
    ]),
    ("Bluesky", &[
        "Reading: nothing to set up, put your handle under Your accounts. Bluesky has no view counts.",
        "Posting: on bsky.app, Settings, Privacy and security, App passwords, add one, and keep it here as \"Bluesky app password\". It can post but can't change your password, and you can revoke it there any time. Every post still waits for your yes.",
    ]),
    ("X, LinkedIn, and the rest", &[
        "X: Settings, Your account, Download an archive. When the email comes, give me the zip below.",
        "LinkedIn: your profile, Posts & activity (Creator analytics), Export. Give me the .xlsx below.",
        "TikTok or Instagram without an app: their \"Download your data\" in JSON works too.",
        "YouTube Studio: Analytics, Advanced mode, Export current view, CSV. Give me Table data.csv below.",
    ]),
];

/// What can't be had, and why -- the same words wherever it's asked.
pub const THE_WALLS: &[(&str, &str)] = &[
    ("X", "no free API since February 2026, and the archive export has likes and reposts but no impressions -- those need Premium, a subscription"),
    ("TikTok", "no watch time or retention for anyone outside TikTok; the data export has your videos, and per-video views only where the export includes them"),
    ("LinkedIn", "analytics only through the export file (Creator analytics, Export); the API needs a partner application"),
    ("Reddit", "its API needs an approval that isn't given to personal use, so subreddit feeds come without scores or comment counts"),
    ("Bluesky", "no view counts at all, and searching posts needs a signed-in account"),
    ("Others' TikTok, Instagram and X", "no free legitimate route: their terms forbid automated collection, and doing it signed in as you risks your own account -- so Atlas opens one page when you ask, and never on a schedule"),
    ("Facebook", "numbers only for a Page; a personal profile has no API for them"),
    ("Threads search", "reading other people's Threads posts by keyword needs Meta's approval; without it, search returns only your own"),
    ("Google Trends", "only the daily trending list; interest over time for a term you choose has no free official route"),
];

/// Days since 1970 for a UTC time, the key every snapshot is filed under.
pub fn social_day(t: u64) -> i64 {
    (t / 86_400) as i64
}

/// "28 Sep" for a day number.
pub fn day_label(day: i64) -> String {
    let c = crate::civil::Civil::from_local(day * 86_400);
    const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{} {}", c.day, M[(c.month as usize).saturating_sub(1).min(11)])
}

/// A count as a person says it: 1,234 and 1.2M.
pub fn count_said(n: u64) -> String {
    if n >= 10_000_000 {
        format!("{:.0}M", n as f64 / 1e6)
    } else if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1e6)
    } else {
        let s = n.to_string();
        let mut out = String::new();
        for (i, c) in s.chars().enumerate() {
            if i > 0 && (s.len() - i).is_multiple_of(3) {
                out.push(',');
            }
            out.push(c);
        }
        out
    }
}

/// What's kept between turns: the record and the watch list, read once;
/// the one-page spacing; when the tick last looked; and what the last
/// scheduled refresh couldn't read, for the Social page.
#[derive(Debug, Default)]
pub struct Live {
    pub(crate) book: Option<snapshots::Book>,
    pub(crate) watch: Option<watchlist::Watch>,
    pub(crate) spacing: onepage::Spacing,
    pub(crate) last_look: u64,
    pub(crate) last_missing: Vec<String>,
}

/// The sites you sign in to yourself, once, in Atlas's own browser (5 Oct
/// 2026: Eric accepted the terms-of-service risk). Name, and the domain its
/// sign-in page is found by (`webrun::login_url`).
pub const SIGN_IN_SITES: &[(&str, &str)] = &[
    ("Instagram", "instagram.com"),
    ("TikTok", "tiktok.com"),
    ("X", "x.com"),
    ("Facebook", "facebook.com"),
    ("LinkedIn", "linkedin.com"),
    ("Reddit", "reddit.com"),
    ("YouTube Studio", "youtube.com"),
];
