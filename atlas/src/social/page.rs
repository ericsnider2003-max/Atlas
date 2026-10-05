//! The hub's Social page (the dashboard pattern: figures across the top,
//! then a section each for your accounts, what you watch, and what can't be
//! had). A pure function of a view the running Atlas fills in (`glue`), so
//! it renders -- and is tested -- without a daemon.

use crate::hub::{esc, shell_at, Page};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlatformRow {
    pub name: String,
    pub posts: usize,
    /// "1,234 on 28 Sep", or empty when the data has none.
    pub followers: String,
    pub latest: String,
    pub source: String,
}

/// Which of your accounts Atlas reads, as set (`workday.social`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Accounts {
    pub youtube_channel: String,
    pub bluesky_handle: String,
    pub instagram: bool,
    pub threads: bool,
    pub facebook_page: bool,
    pub tiktok: bool,
    pub youtube_analytics: bool,
    pub google_app_in_testing: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct View {
    pub notice: Option<String>,
    pub own_refresh: bool,
    pub scan: bool,
    pub rows: Vec<PlatformRow>,
    /// Platforms with nothing kept.
    pub nothing_from: Vec<String>,
    pub last_video: String,
    pub worked: String,
    pub followers: String,
    /// (label, how it's going)
    pub watching: Vec<(String, String)>,
    pub digest: super::analysis::Digest,
    pub summary: Option<String>,
    /// (what, kept in the vault?) -- `None` while the vault is locked.
    pub keys: Vec<(String, Option<bool>)>,
    pub searches_left: u32,
    pub lapsing: Option<String>,
    pub accounts: Accounts,
}

fn tick(name: &str, label: &str, on: bool) -> String {
    format!("<label><input type=checkbox name={name} value=on{}> {}</label> ", if on { " checked" } else { "" }, esc(label))
}

fn form(what: &str, inner: &str, button: &str) -> String {
    format!("<form method=post action='/hub/social' class=inline><input type=hidden name=what value='{what}'>{inner}<button>{}</button></form>", esc(button))
}

pub fn render_social(v: &View) -> String {
    let mut b = String::new();
    if let Some(n) = &v.notice {
        b.push_str(&format!("<p class=notice role=status>{}</p>", esc(n)));
    }
    if let Some(l) = &v.lapsing {
        b.push_str(&format!("<div class='banner wait' role=status>{}</div>", esc(l)));
    }
    b.push_str("<p class=lead>Your accounts' numbers, kept every day so the history outlives the platforms' 28 to 90 days, and what's working for the people and topics you watch. Every figure here came out of the data; where there's none, it says so.</p>");
    let posts: usize = v.rows.iter().map(|r| r.posts).sum();
    b.push_str(&format!(
        "<div class=figures><div class=figure><b>{}</b><span>platforms with data</span></div><div class=figure><b>{posts}</b><span>posts on record</span></div><div class=figure><b>{}</b><span>sources watched</span></div></div>",
        v.rows.len(),
        v.watching.len()
    ));

    // -- your accounts
    b.push_str("<section><h2>Your accounts</h2>");
    if v.rows.is_empty() {
        b.push_str("<p class=nothing>Nothing yet. Import a platform's own export below, or turn on the daily refresh in Settings.</p>");
    } else {
        b.push_str("<table><thead><tr><th scope=col>Platform</th><th scope=col>Posts</th><th scope=col>Followers</th><th scope=col>Latest</th><th scope=col>From</th></tr></thead><tbody>");
        for r in &v.rows {
            b.push_str(&format!(
                "<tr><th scope=row>{}</th><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                esc(&r.name),
                r.posts,
                if r.followers.is_empty() { "not in the data".to_string() } else { esc(&r.followers) },
                esc(&r.latest),
                esc(&r.source)
            ));
        }
        b.push_str("</tbody></table>");
    }
    if !v.nothing_from.is_empty() {
        b.push_str(&format!("<p class=meta>Nothing from {} yet.</p>", esc(&v.nothing_from.join(", "))));
    }
    for (h, text) in [("Your last video", &v.last_video), ("What worked this month", &v.worked), ("Followers", &v.followers)] {
        if !text.is_empty() {
            b.push_str(&format!("<h3>{h}</h3><p>{}</p>", esc(text)));
        }
    }
    b.push_str(&format!(
        "<p class=meta>Daily refresh of your accounts: <b>{}</b>. Scanning what you watch: <b>{}</b>. Both are in <a href='{}'>Settings</a>.</p>",
        if v.own_refresh { "on" } else { "off" },
        if v.scan { "on" } else { "off" },
        Page::Settings.href()
    ));
    b.push_str(&form("refresh", "", "Refresh my numbers now"));
    b.push_str("<h3>Import an export</h3><p>Give the path of the zip, folder or file the platform sent you: the X archive, TikTok's or Instagram's data download (JSON), LinkedIn's analytics .xlsx, or YouTube Studio's Table data.csv.</p>");
    b.push_str(&form("import", "<label>File or folder <input name=path required size=48></label>", "Import it"));
    b.push_str("</section>");

    // -- which accounts
    let a = &v.accounts;
    b.push_str("<section><h2>Which accounts I read</h2><p>Your handles, and which platforms the refresh reads through their own APIs. Each one that needs a key says so below; one that's on without its key is named as not set up, and nothing else stops.</p>");
    b.push_str(&form(
        "accounts",
        &format!(
            "<label>Your YouTube channel (@handle or UC... id) <input name=youtube_channel value='{}' size=24></label> <label>Your Bluesky handle <input name=bluesky_handle value='{}' size=24></label><br>{}{}{}{}{}{}",
            esc(&a.youtube_channel),
            esc(&a.bluesky_handle),
            tick("youtube_analytics", "YouTube retention (Analytics sign-in)", a.youtube_analytics),
            tick("google_app_in_testing", "my Google app is still in Testing", a.google_app_in_testing),
            tick("instagram", "Instagram", a.instagram),
            tick("threads", "Threads", a.threads),
            tick("facebook_page", "Facebook Page", a.facebook_page),
            tick("tiktok", "TikTok", a.tiktok),
        ),
        "Save",
    ));
    b.push_str("</section>");

    // -- keys
    b.push_str("<section><h2>Keys and sign-ins</h2><p>Kept in the vault, never in a file, and only for your own accounts. How to get each one is under \"How to set each one up\" below.</p><ul class=plainlist>");
    for (what, kept) in &v.keys {
        let state = match kept {
            Some(true) => "kept",
            Some(false) => "not set",
            None => "vault locked",
        };
        b.push_str(&format!("<li><span>{}</span><span class=meta>{state}</span></li>", esc(what)));
    }
    b.push_str("</ul>");
    b.push_str(&form(
        "key",
        "<label>Which <select name=name><option value=youtube>YouTube API key</option><option value=instagram>Instagram token</option><option value=threads>Threads token</option><option value=facebook>Facebook Page token</option><option value=bluesky>Bluesky app password (for posting)</option></select></label> <label>Key <input name=secret type=password required autocomplete=off></label>",
        "Keep it",
    ));
    if crate::oauthlink::google_secret().is_some() {
        b.push_str(&form("google", "", "Sign in with Google for YouTube Analytics"));
        b.push_str("<details><summary>Use your own Google app instead</summary>");
    }
    b.push_str(&form(
        "google",
        "<label>Google client ID <input name=id required size=30></label> <label>Client secret <input name=secret type=password required autocomplete=off></label>",
        "Sign in for YouTube Analytics",
    ));
    if crate::oauthlink::google_secret().is_some() {
        b.push_str("</details>");
    }
    b.push_str(&form(
        "tiktok-start",
        "<label>TikTok client key <input name=key required size=20></label> <label>Client secret <input name=secret type=password required autocomplete=off></label> <label>Redirect address <input name=redirect required size=30 placeholder='https://'></label>",
        "Sign in to TikTok",
    ));
    b.push_str(&form(
        "tiktok-finish",
        "<label>The address TikTok sent you to <input name=address required size=48></label>",
        "Finish TikTok sign-in",
    ));
    b.push_str("<h3>Sign in to your accounts</h3><p class=what>Once each, in Atlas's own browser: the window opens on the \
                site's sign-in page, you sign in the way you normally do (codes included) and close it. Atlas keeps that \
                sign-in for reading your pages. The sites don't allow apps to do this, so an account can occasionally be \
                asked to confirm it's you.</p><div class=signins>");
    for (name, domain) in super::SIGN_IN_SITES {
        b.push_str(&form("browser-signin", &format!("<input type=hidden name=site value='{}'>", esc(domain)), &format!("Sign in to {name}")));
    }
    b.push_str("</div>");
    b.push_str("<h3>How to set each one up</h3>");
    for (what, steps) in super::SETUP {
        b.push_str(&format!("<details><summary>{}</summary><ol>", esc(what)));
        for s in steps.iter() {
            b.push_str(&format!("<li>{}</li>", esc(s)));
        }
        b.push_str("</ol></details>");
    }
    b.push_str("</section>");

    // -- watching
    b.push_str("<section><h2>What you watch</h2>");
    if v.watching.is_empty() {
        b.push_str("<p class=nothing>Nothing yet. Add a YouTube @channel, a #hashtag (Mastodon), r/subreddit, a Bluesky handle, a topic, Hacker News, Product Hunt or Google trends.</p>");
    } else {
        b.push_str("<ul class=plainlist>");
        for (label, how) in &v.watching {
            b.push_str(&format!(
                "<li><span>{}</span><span class=meta>{}</span>{}</li>",
                esc(label),
                esc(how),
                form("unwatch", &format!("<input type=hidden name=which value='{}'>", esc(label)), "Stop watching")
            ));
        }
        b.push_str("</ul>");
    }
    b.push_str(&form("watch", "<label>Watch <input name=target required size=32 placeholder='@channel, #tag, r/sub, a topic'></label>", "Add"));
    b.push_str(&format!("<p class=meta>YouTube searches left today: {}.</p>", v.searches_left));
    if v.digest.is_empty() {
        b.push_str("<p class=nothing>Nothing read yet.</p>");
    } else {
        if let Some(s) = &v.summary {
            b.push_str(&format!("<p>{}</p>", esc(s)));
        }
        for (h, lines) in &v.digest.sections {
            b.push_str(&format!("<h3>{}</h3><ul class=plainlist>", esc(h)));
            for l in lines {
                b.push_str(&format!("<li>{}</li>", esc(l)));
            }
            b.push_str("</ul>");
        }
    }
    if !v.digest.gaps.is_empty() {
        b.push_str(&format!("<p class=meta>Not read: {}</p>", esc(&v.digest.gaps.join("; "))));
    }
    b.push_str(&form("scan", "", "Read them now"));
    b.push_str(&form("summarise", "", "Sum it up"));
    b.push_str("</section>");

    // -- the walls
    b.push_str("<section><h2>What can't be had, and why</h2><ul class=plainlist>");
    for (who, why) in super::THE_WALLS {
        b.push_str(&format!("<li><b>{}</b>: {}</li>", esc(who), esc(why)));
    }
    b.push_str("</ul></section>");
    shell_at(Some(Page::Social), "Social", &b)
}
