//! The daemon side of `social`: what you say, the tick, the crew errands,
//! the brief, and the hub page's buttons. The modules beside this one know
//! nothing of the daemon; this is the one place that does.
//!
//! - **Asking never waits on the network**: answers come from what's kept.
//!   Anything that goes out -- a refresh, a scan, a page, a summary, a
//!   Google sign-in -- is a crew errand, and says how it went when it ends.
//! - **Nothing fetches on a schedule unless you turned it on**
//!   (`own_refresh`, `scan`). An ask is you choosing to go out, so it works
//!   either way.
//! - **Keys are read on the tick side, from the vault**, and handed to the
//!   errand; a locked vault means the sources that need no key, said.

use super::{analysis, page};
use super::apis::{self, GoogleSignIn, Https, TikTokSignIn};
use super::onepage;
use super::snapshots::{Book, Platform, Record};
use super::watchlist::{self, Seen, Target, Watch};
use super::{SocialConfig, VAULT_BLUESKY, VAULT_FACEBOOK, VAULT_INSTAGRAM, VAULT_THREADS, VAULT_TIKTOK, VAULT_YOUTUBE_KEY, VAULT_YOUTUBE_OAUTH};
use crate::daemon::Daemon;
use serde::{Deserialize, Serialize};

/// Where a person ends an app's access to their Facebook, Instagram or Threads account.
const META_APPS: &str = "facebook.com/settings?tab=business_integrations";

/// Sources read in one scheduled errand.
const PER_SCAN: usize = 5;
/// Sources read when you ask.
const PER_ASK: usize = 20;
/// Two seconds between requests to one host.
const HOST_SPACING_MS: u64 = 2000;

/// What the refresh errand brings back.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Refreshed {
    records: Vec<Record>,
    done: Vec<String>,
    missing: Vec<String>,
    new_instagram_token: Option<String>,
    new_threads_token: Option<String>,
    /// TikTok hands out a new refresh token at each use; the old one stops.
    new_tiktok: Option<TikTokSignIn>,
}

/// One source's read, carried back from the scan errand.
#[derive(Debug, Serialize, Deserialize)]
struct ScanOut {
    key: String,
    items: Vec<Seen>,
    last_modified: Option<String>,
    unchanged: bool,
    resolved: Option<Target>,
    error: Option<String>,
    retry_after: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Summary {
    text: String,
    facts: String,
    took_ms: u64,
    prompt_chars: usize,
    failed: Option<String>,
}

/// A path in what you said: quoted, or the first word that looks like one.
fn path_in(said: &str) -> Option<String> {
    if let Some(a) = said.find('"') {
        if let Some(b) = said[a + 1..].find('"') {
            return Some(said[a + 1..a + 1 + b].to_string());
        }
    }
    let exts = [".zip", ".xlsx", ".csv", ".json", ".js"];
    let words: Vec<&str> = said.split_whitespace().collect();
    // A path can hold spaces: from the first word that starts like one to
    // the last that ends like one.
    let start = words.iter().position(|w| w.starts_with('/') || w.starts_with('~') || w.chars().nth(1) == Some(':') || w.starts_with("\\\\"))?;
    let end = words.iter().rposition(|w| exts.iter().any(|e| w.to_lowercase().ends_with(e))).filter(|e| *e >= start).unwrap_or(start);
    Some(words[start..=end].join(" ").trim_end_matches(['.', ',']).to_string())
}

fn has(low: &str, any: &[&str]) -> bool {
    any.iter().any(|w| low.contains(w))
}

impl Daemon<'_> {
    /// Every site you sign in to yourself, in one window of Atlas's own
    /// browser, a tab each: sign in to the ones you use, close the window.
    /// What the Social page's one button and "sign me into my socials" do.
    pub(crate) fn open_social_signins(&mut self) -> String {
        let urls: Vec<String> = super::SIGN_IN_SITES.iter().map(|(_, d)| crate::webrun::login_url(d)).collect();
        let bcfg = self.tools_cfg().browser.clone();
        let vars = self.tools_cfg().vars.clone();
        match crate::browser::open_sign_in_window(&bcfg, &vars, &urls) {
            Ok(()) => format!(
                "Atlas's browser is open with a tab for each site ({}). Sign in on the ones you use, the way you normally do, \
                 and skip the rest; close the window when you're done. Atlas keeps those sign-ins.",
                super::SIGN_IN_SITES.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")
            ),
            Err(e) => format!("Atlas's browser didn't open: {e}"),
        }
    }

    pub(crate) fn social_cfg(&self) -> SocialConfig {
        self.workday_cfg().social
    }

    fn social_book(&mut self) -> &mut Book {
        let path = self.store.root().join(super::snapshots::FILE);
        self.workday.social.book.get_or_insert_with(|| Book::load(&path))
    }

    fn social_watch(&mut self) -> &mut Watch {
        let store = &self.store;
        self.workday.social.watch.get_or_insert_with(|| store.load(watchlist::FILE))
    }

    fn social_keep_watch(&mut self) -> Result<(), String> {
        let w = self.social_watch().clone();
        self.store.save(watchlist::FILE, &w).map_err(|e| e.to_string())
    }

    fn social_add(&mut self, records: Vec<Record>) -> Result<super::snapshots::Added, String> {
        let path = self.store.root().join(super::snapshots::FILE);
        self.social_book().add(&path, records).map_err(|e| format!("I couldn't write it down: {e}"))
    }

    /// A key from the vault: `Ok(None)` when it isn't there, `Err` when the
    /// vault is locked.
    /// Your Google sign-in for YouTube, when there is one that still refreshes.
    fn youtube_signin(&mut self, t: u64) -> Option<GoogleSignIn> {
        self.social_secret(VAULT_YOUTUBE_OAUTH, t).ok().flatten().and_then(|j| serde_json::from_str::<GoogleSignIn>(&j).ok()).filter(|g| !g.refresh_token.is_empty())
    }

    fn social_secret(&mut self, name: &str, t: u64) -> Result<Option<String>, String> {
        if self.vault_ready(t).is_err() {
            return Err("the vault is locked".into());
        }
        Ok(self.vault.get(name, t).ok())
    }

    // ------------------------------------------------------------ what you say

    pub(crate) fn social_said(&mut self, said: &str, t: u64) -> String {
        let cfg = self.social_cfg();
        if !cfg.enabled {
            return "Your social accounts are switched off (workday.social.enabled).".into();
        }
        let low = said.to_ascii_lowercase();
        let only = Platform::in_words(&low);
        if let Some(url) = onepage::url_in(said) {
            if onepage::check_url(&url).is_ok() {
                return self.social_read_page(&url, t);
            }
        }
        if low.contains("import") {
            return match path_in(said) {
                Some(p) => self.social_import(&p, t),
                None => "Import what? Give me the path of the export -- the zip, the folder, or the file -- or use the Social page.".into(),
            };
        }
        if has(&low, &["stop watching", "unwatch", "stop keeping an eye on", "stop following"]) {
            let what = ["stop watching", "unwatch", "stop keeping an eye on", "stop following"].iter().find_map(|p| low.find(p).map(|i| said[i + p.len()..].to_string())).unwrap_or_default();
            let gone = self.social_watch().remove(&what);
            return if gone.is_empty() {
                format!("I'm not watching anything matching \"{}\".", what.trim())
            } else {
                match self.social_keep_watch() {
                    Ok(()) => format!("Stopped watching {}.", gone.join(", ")),
                    Err(e) => format!("Stopped watching {}, but I couldn't save that: {e}", gone.join(", ")),
                }
            };
        }
        if has(&low, &["what am i watching", "my watch list", "watch list", "watchlist"]) {
            let w = self.social_watch();
            if w.list.is_empty() {
                return "You're not watching anything yet. Say \"watch\" and a YouTube @channel, a #hashtag, r/subreddit, a Bluesky handle or a topic.".into();
            }
            return w.list.iter().enumerate().map(|(i, x)| format!("{}. {}", i + 1, x.target.label())).collect::<Vec<_>>().join("\n");
        }
        if has(&low, &["what can't", "what cant", "impossible", "why can't you", "why cant you"]) {
            return super::THE_WALLS.iter().map(|(w, why)| format!("{w}: {why}.")).collect::<Vec<_>>().join("\n");
        }
        // "follow the #rust hashtag on Mastodon" is watching too; following a
        // site's feed is `feeds`, which the parser sends there first.
        if (has(&low, &["watch ", "keep an eye on"]) && !has(&low, &["watch time", "watched"])) || low.trim_start().starts_with("follow ") {
            return self.social_watch_add(said, t);
        }
        if has(&low, &["trending", "digest", "working for them", "what's working for", "whats working for", "on my watch"]) {
            return self.social_digest_said(t);
        }
        if has(&low, &["refresh", "update my numbers", "fetch my numbers", "pull my numbers"]) {
            return self.social_refresh(t, true);
        }
        let book = self.social_book().clone();
        // "my last YouTube video" names the platform between "last" and
        // "video"; read without it so it still lands here (selftest, 3 Oct
        // 2026: it fell through to the overview).
        let bare = ["youtube ", "instagram ", "tiktok ", "linkedin ", "bluesky ", "x ", "twitter "]
            .iter()
            .fold(low.clone(), |s, p| s.replace(&format!("last {p}"), "last ").replace(&format!("latest {p}"), "latest "));
        if has(&bare, &["last video", "latest video", "last post", "latest post", "last reel", "last short", "last tiktok"]) {
            return analysis::last_video(&book, only, t);
        }
        if has(&low, &["time", "when should i post", "when to post", "what day"]) {
            return analysis::posting_times(&book, only, &self.home_zone(), 3);
        }
        if has(&low, &["follower", "subscriber", "audience"]) {
            return analysis::followers(&book, only, t);
        }
        if has(&low, &["worked", "why", "best post", "best video", "did well", "doing well", "performing"]) {
            let days = if low.contains("week") { 7 } else if low.contains("year") { 365 } else { 30 };
            let min = self.tools_ref().map(|x| x.content.min_posts_for_patterns).unwrap_or(8);
            return analysis::what_worked(&book, only, days, t, min);
        }
        analysis::accounts_overview(&book)
    }

    fn social_import(&mut self, path: &str, t: u64) -> String {
        let p = std::path::PathBuf::from(path.trim());
        let got = match super::exports::import_path(&p, t) {
            Ok(g) => g,
            Err(e) => return e,
        };
        let (platform, what, posts, days, missing) = (got.platform, got.what, got.posts, got.account_days, got.missing.clone());
        match self.social_add(got.records) {
            Ok(a) => {
                let mut s = format!(
                    "Read {what}: {} and {} of account figures{}.",
                    if posts == 1 { "1 post".to_string() } else { format!("{posts} posts") },
                    if days == 1 { "1 day".to_string() } else { format!("{days} days") },
                    if a.unchanged > 0 { format!(" ({} already on record, unchanged)", a.unchanged) } else { String::new() }
                );
                if !missing.is_empty() {
                    s.push_str(&format!(" Not in it: {}.", missing.join("; ")));
                }
                s.push_str(&format!(" Ask \"how did my last {} post do\" or \"what worked this month\".", platform.name()));
                s
            }
            Err(e) => e,
        }
    }

    fn social_watch_add(&mut self, said: &str, t: u64) -> String {
        let cfg = self.social_cfg();
        let targets = match watchlist::parse_target(said, &cfg) {
            Ok(x) => x,
            Err(e) => return e,
        };
        let has_key = matches!(self.social_secret(VAULT_YOUTUBE_KEY, t), Ok(Some(_))) || self.youtube_signin(t).is_some();
        let search_without_key = targets.iter().any(|x| matches!(x, Target::YoutubeSearch { .. })) && !has_key;
        let added = match self.social_watch().add(targets, t) {
            Ok(a) => a,
            Err(e) => return e,
        };
        if added.is_empty() {
            return "You're already watching that.".into();
        }
        let kept = self.social_keep_watch();
        let mut s = format!("Watching {}.", added.join(" and "));
        if search_without_key {
            s.push_str(" The YouTube side needs YouTube connected (Connect YouTube on the Social page) -- until then only Hacker News is read for it.");
        }
        s.push_str(if cfg.scan { " I'll read it on the next pass." } else { " Scanning on a schedule is off -- say \"what's trending\" to read it now, or turn on \"Watching others\" in Settings." });
        if let Err(e) = kept {
            s.push_str(&format!(" (I couldn't save the list: {e})"));
        }
        s
    }

    // ------------------------------------------------------------ errands

    /// Your own accounts, from their official APIs, on the crew.
    fn social_refresh(&mut self, t: u64, asked: bool) -> String {
        let cfg = self.social_cfg();
        let mut missing: Vec<String> = Vec::new();
        let secret = |d: &mut Self, name: &str, what: &str, missing: &mut Vec<String>| -> Option<String> {
            match d.social_secret(name, t) {
                Ok(Some(k)) => Some(k),
                Ok(None) => {
                    missing.push(format!("{what} (no key in the vault -- the Social page takes it)"));
                    None
                }
                Err(e) => {
                    missing.push(format!("{what} ({e})"));
                    None
                }
            }
        };
        // YouTube: your Google sign-in is the way in (one Connect button);
        // an API key, where one was kept before, still works.
        let google = self.youtube_signin(t);
        let kept_key = self.social_secret(VAULT_YOUTUBE_KEY, t).ok().flatten();
        let yt_key = match (&kept_key, &google) {
            (_, Some(_)) => kept_key.clone(),
            (Some(_), None) if cfg.youtube_channel.trim().is_empty() => {
                missing.push("YouTube (no channel set: workday.social.youtube_channel)".into());
                None
            }
            (Some(_), None) => kept_key.clone(),
            (None, None) => {
                if cfg.youtube_analytics || !cfg.youtube_channel.trim().is_empty() {
                    missing.push(if self.vault.state() == crate::vault::State::Open {
                        "YouTube (not connected -- Connect YouTube on the Social page)".to_string()
                    } else {
                        "YouTube (the vault is locked)".to_string()
                    });
                }
                None
            }
        };
        let yt_wanted = yt_key.is_some() || google.is_some();
        let ig = if cfg.instagram { secret(self, VAULT_INSTAGRAM, "Instagram", &mut missing) } else { None };
        let ig_refresh_due = t.saturating_sub(self.social_watch().instagram_token_at) > 30 * 86_400;
        let threads = if cfg.threads { secret(self, VAULT_THREADS, "Threads", &mut missing) } else { None };
        let threads_refresh_due = t.saturating_sub(self.social_watch().threads_token_at) > 30 * 86_400;
        let fb = if cfg.facebook_page { secret(self, VAULT_FACEBOOK, "Facebook Page", &mut missing) } else { None };
        let tiktok = if cfg.tiktok {
            secret(self, VAULT_TIKTOK, "TikTok", &mut missing).and_then(|j| serde_json::from_str::<TikTokSignIn>(&j).ok()).filter(|s| !s.refresh_token.is_empty())
        } else {
            None
        };
        if cfg.tiktok && tiktok.is_none() && !missing.iter().any(|m| m.starts_with("TikTok")) {
            missing.push("TikTok (the sign-in wasn't finished -- paste the address TikTok sent you to on the Social page)".into());
        }
        let bsky = cfg.bluesky_handle.trim().to_string();
        if !yt_wanted && ig.is_none() && threads.is_none() && fb.is_none() && tiktok.is_none() && bsky.is_empty() {
            let why = if missing.is_empty() { "nothing is set up".to_string() } else { missing.join("; ") };
            return format!("There's no account I can read from its API yet: {why}. Imports from the platforms' own export files work without any of this.");
        }
        let channel = cfg.youtube_channel.clone();
        let testing = cfg.google_app_in_testing;
        self.social_watch().last_refresh = t;
        crate::heard!(self.social_keep_watch());
        let work: crate::crew::Work = Box::new(move |ctl| {
            let net = Https;
            let mut out = Refreshed { missing, ..Default::default() };
            // Signed in and no key: the sign-in's access token reads the channel too.
            let yt_auth = match (&yt_key, &google) {
                (Some(k), _) => Some(k.clone()),
                (None, Some(g)) => match apis::google_access(&net, g) {
                    Ok(tok) => Some(format!("{}{tok}", apis::BEARER)),
                    Err(e) => {
                        out.missing.push(format!("YouTube ({e})"));
                        None
                    }
                },
                (None, None) => None,
            };
            if let Some(key) = &yt_auth {
                match apis::youtube_own(&net, key, &channel, t) {
                    Ok(mut recs) => {
                        let n = recs.iter().filter(|r| matches!(r, Record::Post(_))).count();
                        if let Some(g) = &google {
                            match apis::google_access(&net, g).and_then(|tok| apis::youtube_analytics(&net, &tok, super::social_day(t) - 28, super::social_day(t) - 1)) {
                                Ok(rows) => {
                                    for r in recs.iter_mut() {
                                        if let Record::Post(p) = r {
                                            if let Some((_, m)) = rows.iter().find(|(id, _)| *id == p.id) {
                                                p.m.fill_from(m);
                                            }
                                        }
                                    }
                                    out.done.push("YouTube retention (the last 28 days)".into());
                                }
                                Err(e) => out.missing.push(format!("YouTube retention ({e})")),
                            }
                            if let Some(l) = g.lapsing(t, testing) {
                                out.missing.push(l);
                            }
                        }
                        out.done.push(format!("YouTube, {n} videos"));
                        out.records.extend(recs);
                    }
                    Err(e) => out.missing.push(format!("YouTube ({e})")),
                }
            }
            if ctl.checkpoint() {
                return Err("stopped".into());
            }
            if let Some(token) = &ig {
                match apis::instagram_own(&net, token, t) {
                    Ok(recs) => {
                        out.done.push(format!("Instagram, {} posts", recs.len().saturating_sub(1)));
                        out.records.extend(recs);
                        if ig_refresh_due {
                            if let Ok(tok) = apis::instagram_refresh(&net, token) {
                                out.new_instagram_token = Some(tok);
                            }
                        }
                    }
                    Err(e) => out.missing.push(format!("Instagram ({e})")),
                }
            }
            if let Some(token) = &threads {
                match apis::threads_own(&net, token, t) {
                    Ok(recs) => {
                        out.done.push(format!("Threads, {} posts", recs.len().saturating_sub(1)));
                        out.records.extend(recs);
                        if threads_refresh_due {
                            if let Ok(tok) = apis::threads_refresh(&net, token) {
                                out.new_threads_token = Some(tok);
                            }
                        }
                    }
                    Err(e) => out.missing.push(format!("Threads ({e})")),
                }
            }
            if let Some(token) = &fb {
                match apis::facebook_page_own(&net, token, t) {
                    Ok(recs) => {
                        out.done.push(format!("your Facebook Page, {} posts", recs.len().saturating_sub(1)));
                        out.records.extend(recs);
                    }
                    Err(e) => out.missing.push(format!("Facebook Page ({e})")),
                }
            }
            if let Some(s) = &tiktok {
                match apis::tiktok_access(&net, s) {
                    Ok((access, refresh)) => {
                        out.new_tiktok = Some(TikTokSignIn { refresh_token: refresh, obtained: t, ..s.clone() });
                        match apis::tiktok_own(&net, &access, t) {
                            Ok(recs) => {
                                out.done.push(format!("TikTok, {} videos", recs.len().saturating_sub(1)));
                                out.records.extend(recs);
                            }
                            Err(e) => out.missing.push(format!("TikTok ({e})")),
                        }
                    }
                    Err(e) => out.missing.push(format!("TikTok ({e})")),
                }
            }
            if ctl.checkpoint() {
                return Err("stopped".into());
            }
            if !bsky.is_empty() {
                match apis::bluesky_own(&net, &bsky, t) {
                    Ok(recs) => {
                        out.done.push(format!("Bluesky, {} posts", recs.len().saturating_sub(1)));
                        out.records.extend(recs);
                    }
                    Err(e) => out.missing.push(format!("Bluesky ({e})")),
                }
            }
            serde_json::to_string(&out).map_err(|e| e.to_string())
        });
        let name = if asked { "social-refresh-asked" } else { "social-refresh" };
        match self.hand_off_social(name, t, work, None, asked) {
            Some(_) => "Reading your accounts' numbers now; I'll say when it's done.".into(),
            None => "I've too much on to read your accounts right now. Ask again in a minute.".into(),
        }
    }

    /// Read watched sources on the crew: the due ones, or all of them when
    /// asked.
    fn social_scan(&mut self, t: u64, asked: bool) -> Option<String> {
        let cfg = self.social_cfg();
        let yt_key = self.social_secret(VAULT_YOUTUBE_KEY, t).ok().flatten();
        let google = if yt_key.is_none() { self.youtube_signin(t) } else { None };
        let has_youtube = yt_key.is_some() || google.is_some();
        let every = cfg.scan_every_minutes.max(30);
        let pday = watchlist::pacific_day(t);
        let budget = cfg.youtube_searches_per_day;
        let w = self.social_watch();
        let picks: Vec<usize> = if asked { (0..w.list.len()).take(PER_ASK).collect() } else { w.due(t).into_iter().take(PER_SCAN).collect() };
        if picks.is_empty() {
            return asked.then(|| "You're not watching anything yet.".into());
        }
        let mut jobs: Vec<(Target, Option<String>, bool)> = Vec::new();
        for i in picks {
            let search = matches!(w.list[i].target, Target::YoutubeSearch { .. });
            // A search spends today's budget only when there's a key to spend it with.
            let may = search && has_youtube && w.quota.take(pday, budget);
            // Marked later now, so a slow read isn't handed out twice.
            w.list[i].next_due = t + every * 60;
            jobs.push((w.list[i].target.clone(), w.list[i].last_modified.clone(), may));
        }
        crate::heard!(self.social_keep_watch());
        let work: crate::crew::Work = Box::new(move |ctl| {
            let net = Https;
            let mut gap = crate::ratelimit::Gcra::new(1, HOST_SPACING_MS, 1);
            let mut out: Vec<ScanOut> = Vec::new();
            // Your Google sign-in stands in for an API key, fetched once a pass.
            let yt_key = yt_key.or_else(|| google.as_ref().and_then(|g| apis::google_access(&net, g).ok()).map(|tok| format!("{}{tok}", apis::BEARER)));
            for (target, lm, may) in jobs {
                if ctl.checkpoint() {
                    break;
                }
                let host = target.key().split(':').next().unwrap_or("").to_string();
                loop {
                    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
                    match gap.check(&host, now_ms) {
                        Ok(()) => break,
                        Err(wait) => std::thread::sleep(std::time::Duration::from_millis(wait.min(HOST_SPACING_MS))),
                    }
                }
                let key = target.key();
                out.push(match watchlist::read_one(&net, &target, lm.as_deref(), yt_key.as_deref(), may, t) {
                    Ok(f) => ScanOut { key, items: f.items, last_modified: f.last_modified, unchanged: f.unchanged, resolved: f.resolved, error: None, retry_after: None },
                    Err(e) => ScanOut { key, items: Vec::new(), last_modified: None, unchanged: false, resolved: None, error: Some(e.why), retry_after: e.retry_after },
                });
            }
            serde_json::to_string(&out).map_err(|e| e.to_string())
        });
        let name = if asked { "social-scan-asked" } else { "social-scan" };
        let taken = self.hand_off_social(name, t, work, None, asked).is_some();
        asked.then(|| if taken { "Reading what you watch now; I'll tell you what stands out.".into() } else { "I've too much on to read them right now. Ask again in a minute.".into() })
    }

    /// "What's trending?": what's kept now, and a fresh read on the crew.
    fn social_digest_said(&mut self, t: u64) -> String {
        let w = self.social_watch().clone();
        if w.list.is_empty() {
            return "You're not watching anything yet. Say \"watch\" and a YouTube @channel, a #hashtag, r/subreddit, a Bluesky handle or a topic.".into();
        }
        let d = analysis::watch_digest(&w, t);
        let fresh = w.list.iter().any(|x| x.last_ok.is_some_and(|ok| t.saturating_sub(ok) < 3600));
        if d.is_empty() || !fresh {
            return self.social_scan(t, true).unwrap_or_default();
        }
        let mut s = d.sections.iter().map(|(h, lines)| format!("{h}: {}", lines.join("; "))).collect::<Vec<_>>().join("\n");
        if self.llm.is_some() {
            if let Some(ok) = self.social_summarise(t, &d, true) {
                s.push_str(&format!("\n{ok}"));
            }
        }
        s
    }

    /// A summary by the local model, checked number by number against the
    /// digest before it's used.
    fn social_summarise(&mut self, t: u64, d: &analysis::Digest, asked: bool) -> Option<String> {
        let llm = self.llm.clone()?;
        let facts = d.facts();
        let work: crate::crew::Work = Box::new(move |_ctl| {
            let started = std::time::Instant::now();
            let r = llm.complete(analysis::SUMMARY_SYSTEM, &facts);
            let took_ms = started.elapsed().as_millis() as u64;
            let (text, failed) = match r {
                Ok(x) => (x, None),
                Err(e) => (String::new(), Some(e.to_string())),
            };
            serde_json::to_string(&Summary { text, prompt_chars: facts.len() + analysis::SUMMARY_SYSTEM.len(), facts, took_ms, failed }).map_err(|e| e.to_string())
        });
        let name = if asked { "social-summary" } else { "social-summary-quiet" };
        self.hand_off_social(name, t, work, None, asked).map(|_| "I'm asking the model to sum it up too.".to_string())
    }

    /// One TikTok, Instagram or X page, in Atlas's own browser, once.
    fn social_read_page(&mut self, url: &str, t: u64) -> String {
        let (site, url) = match onepage::check_url(url) {
            Ok(x) => x,
            Err(e) => return e,
        };
        if let Err(wait) = self.workday.social.spacing.may(site, t) {
            return format!("I read a {} page a moment ago. One every two minutes -- ask again in {wait} seconds.", site.name());
        }
        let bcfg = self.tools_cfg().browser.clone();
        let vars = self.tools_cfg().vars.clone();
        let u = url.clone();
        let work: crate::crew::Work = Box::new(move |ctl| {
            let mut b = crate::browser::Browser::start(&bcfg, &vars).map_err(|e| format!("Atlas's browser didn't start: {e}"))?;
            let got = (|| -> Result<String, String> {
                b.open(&u).map_err(|e| e.to_string())?;
                // Read until the page stops growing (its scripts draw it),
                // three looks at most.
                let mut text = String::new();
                for _ in 0..3 {
                    std::thread::sleep(std::time::Duration::from_millis(1500));
                    if ctl.stopping() {
                        break;
                    }
                    let now = b.read().map_err(|e| e.to_string())?;
                    let settled = now.len() == text.len() && !now.is_empty();
                    text = now;
                    if settled {
                        break;
                    }
                }
                Ok(onepage::said_about(site, &u, &text))
            })();
            b.close();
            got
        });
        match self.hand_off_social("social-read", t, work, Some(url.clone()), true) {
            Some(_) => format!("Opening that {} page once in my own browser.", site.name()),
            None => "I've too much on to open it right now. Ask again in a minute.".into(),
        }
    }

    /// Google sign-in for YouTube Analytics: your browser, Google's page,
    /// back to a port on this machine.
    fn social_google(&mut self, client_id: &str, secret: &str, t: u64) -> String {
        if let Err(e) = self.vault_ready(t) {
            return format!("Nothing was started: {e}.");
        }
        let listener = match std::net::TcpListener::bind("127.0.0.1:0") {
            Ok(l) => l,
            Err(e) => return format!("I couldn't open a port for Google to come back to: {e}"),
        };
        let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
        let redirect = format!("http://127.0.0.1:{port}");
        let verifier = crate::vault::short_code(60);
        let state = crate::vault::short_code(16);
        let url = apis::google_consent_url(client_id, &redirect, &state, &apis::pkce_challenge(&verifier));
        if let Err(e) = self.plat.open_path(&url) {
            return format!("I couldn't open your browser for Google's sign-in: {e}");
        }
        let s = GoogleSignIn { client_id: client_id.trim().into(), client_secret: secret.trim().into(), refresh_token: String::new(), obtained: 0 };
        let work: crate::crew::Work = Box::new(move |ctl| {
            use std::io::{BufRead, Write};
            let _ = listener.set_nonblocking(true);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
            loop {
                if ctl.stopping() || std::time::Instant::now() > deadline {
                    return Err("the Google sign-in wasn't finished within five minutes".into());
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
                        let mut reader = std::io::BufReader::new(&stream);
                        let mut line = String::new();
                        crate::heard!(reader.read_line(&mut line));
                        let code = apis::code_from_redirect(&line, &state);
                        let page = if code.is_ok() { "Signed in. You can close this tab and go back to Atlas." } else { "That didn't work -- go back to Atlas to see why." };
                        let mut w = &stream;
                        let _ = write!(w, "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{page}", page.len());
                        let code = code?;
                        let refresh = apis::google_exchange(&Https, &s, &code, &redirect, &verifier)?;
                        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                        return serde_json::to_string(&GoogleSignIn { refresh_token: refresh, obtained: now, ..s }).map_err(|e| e.to_string());
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(std::time::Duration::from_millis(200)),
                    Err(e) => return Err(format!("the sign-in's port failed: {e}")),
                }
            }
        });
        match self.hand_off_social("social-google", t, work, None, true) {
            Some(_) => "Google's sign-in is open in your browser. Say yes to read-only YouTube Analytics and come back.".into(),
            None => "I've too much on to start the sign-in. Try again in a minute.".into(),
        }
    }

    /// What the crew brought back.
    pub(crate) fn social_news(&mut self, label: &str, ending: &crate::crew::Ending, t: u64) -> Option<String> {
        use crate::crew::Ending;
        let asked = label.ends_with("-asked") || matches!(label, "social-read" | "social-summary" | "social-google" | "social-tiktok");
        let body = match ending {
            Ending::Done(Ok(b)) => b.clone(),
            Ending::Done(Err(e)) => return asked.then_some(match label {
                "social-read" => format!("I couldn't read that page: {e}"),
                "social-google" => format!("The Google sign-in didn't finish: {e}"),
                "social-tiktok" => format!("The TikTok sign-in didn't finish: {e}"),
                _ => format!("That didn't work: {e}"),
            }),
            Ending::Stopped => return asked.then(|| "Stopped.".into()),
            Ending::Vanished => return asked.then(|| "That stopped without finishing. Ask me again.".into()),
        };
        match label {
            "social-read" => Some(body),
            "social-refresh" | "social-refresh-asked" => {
                let r: Refreshed = serde_json::from_str(&body).ok()?;
                if let Some(tok) = &r.new_instagram_token {
                    if self.vault.put(VAULT_INSTAGRAM, crate::vault::Kind::ApiKey, tok, t).is_ok() {
                        crate::kept!(self.vault.save(&self.vault_home));
                        self.social_watch().instagram_token_at = t;
                        crate::heard!(self.social_keep_watch());
                    }
                }
                if let Some(tok) = &r.new_threads_token {
                    if self.vault.put(VAULT_THREADS, crate::vault::Kind::ApiKey, tok, t).is_ok() {
                        crate::kept!(self.vault.save(&self.vault_home));
                        self.social_watch().threads_token_at = t;
                        crate::heard!(self.social_keep_watch());
                    }
                }
                if let Some(tk) = &r.new_tiktok {
                    if let Ok(j) = serde_json::to_string(tk) {
                        if self.vault.put(VAULT_TIKTOK, crate::vault::Kind::ApiKey, &j, t).is_ok() {
                            crate::kept!(self.vault.save(&self.vault_home));
                        }
                    }
                }
                let kept = self.social_add(r.records);
                self.workday.social.last_missing = r.missing.clone();
                if !asked {
                    return None;
                }
                let mut s = match kept {
                    Ok(_) if r.done.is_empty() => "I couldn't read any of your accounts.".to_string(),
                    Ok(_) => format!("Read {}.", r.done.join(", ")),
                    Err(e) => e,
                };
                if !r.missing.is_empty() {
                    s.push_str(&format!(" Not read: {}.", r.missing.join("; ")));
                }
                Some(s)
            }
            "social-scan" | "social-scan-asked" => {
                let outs: Vec<ScanOut> = serde_json::from_str(&body).ok()?;
                let every = self.social_cfg().scan_every_minutes;
                let mut failed = Vec::new();
                let w = self.social_watch();
                for o in outs {
                    match o.error {
                        None => w.took(&o.key, watchlist::Fetched { items: o.items, last_modified: o.last_modified, unchanged: o.unchanged, resolved: o.resolved }, t, every),
                        Some(why) => {
                            failed.push(format!("{} ({why})", w.list.iter().find(|x| x.target.key() == o.key).map(|x| x.target.label()).unwrap_or(o.key.clone())));
                            w.failed(&o.key, &watchlist::Failed { why, retry_after: o.retry_after }, t, every);
                        }
                    }
                }
                crate::heard!(self.social_keep_watch());
                if !asked {
                    // Once a day, the model sums up what the scans found, for
                    // the morning brief; checked like any other summary.
                    let stale = self.social_watch().summary.as_ref().is_none_or(|(at, _)| t.saturating_sub(*at) >= 20 * 3600);
                    if stale && self.llm.is_some() {
                        let d = analysis::watch_digest(self.social_watch(), t);
                        if !d.is_empty() {
                            // unheard-ok: returns `Option<String>`, not a Result
                            let _ = self.social_summarise(t, &d, false);
                        }
                    }
                    return None;
                }
                let d = analysis::watch_digest(self.social_watch(), t);
                let mut s = if d.is_empty() {
                    "Read them; nothing stands out yet.".to_string()
                } else {
                    d.sections.iter().map(|(h, lines)| format!("{h}: {}", lines.join("; "))).collect::<Vec<_>>().join("\n")
                };
                if !failed.is_empty() {
                    s.push_str(&format!("\nCouldn't read: {}.", failed.join("; ")));
                }
                if self.llm.is_some() && !d.is_empty() {
                    if let Some(more) = self.social_summarise(t, &d, true) {
                        s.push_str(&format!("\n{more}"));
                    }
                }
                Some(s)
            }
            "social-summary" | "social-summary-quiet" => {
                let quiet = label == "social-summary-quiet";
                let sm: Summary = serde_json::from_str(&body).ok()?;
                self.record_model_call("social-summary", sm.took_ms, sm.prompt_chars, sm.text.len(), sm.failed.clone());
                if sm.failed.is_some() || sm.text.trim().is_empty() {
                    return (!quiet).then(|| "The model couldn't sum it up; the figures above are what there is.".into());
                }
                if !analysis::grounded(&sm.text, &sm.facts) {
                    return (!quiet).then(|| "The model's summary brought a figure that isn't in what was read, so I've left it out -- the figures above are the real ones.".into());
                }
                self.social_watch().summary = Some((t, sm.text.trim().to_string()));
                crate::heard!(self.social_keep_watch());
                (!quiet).then(|| sm.text.trim().to_string())
            }
            "social-tiktok" => {
                let s: TikTokSignIn = serde_json::from_str(&body).ok()?;
                let json = serde_json::to_string(&s).ok()?;
                Some(match self.vault.put(VAULT_TIKTOK, crate::vault::Kind::ApiKey, &json, t) {
                    Ok(()) => {
                        crate::kept!(self.vault.save(&self.vault_home));
                        let on = if self.social_cfg().tiktok { "" } else { " Turn TikTok on under Your accounts on the Social page so the refresh reads it." };
                        format!("Signed in to TikTok; your videos' numbers come with the next refresh.{on}")
                    }
                    Err(e) => format!("TikTok signed you in, but I couldn't keep it in the vault: {e}"),
                })
            }
            "social-google" => {
                let mut s: GoogleSignIn = serde_json::from_str(&body).ok()?;
                s.obtained = s.obtained.max(1);
                let json = serde_json::to_string(&s).ok()?;
                Some(match self.vault.put(VAULT_YOUTUBE_OAUTH, crate::vault::Kind::ApiKey, &json, t) {
                    Ok(()) => {
                        crate::connecting::note_google_data(self);
                        crate::kept!(self.vault.save(&self.vault_home));
                        let lapse = if self.social_cfg().google_app_in_testing {
                            " While your Google app is in Testing, Google ends this sign-in after seven days; publishing the app (unverified is fine for your own use) stops that."
                        } else {
                            ""
                        };
                        let mut on = String::new();
                        if !self.social_cfg().own_refresh {
                            on = format!(" {}", self.apply_setting("workday.social.own_refresh", "on"));
                        }
                        format!("YouTube is connected; your channel's numbers and retention come with the next refresh.{lapse}{on}")
                    }
                    Err(e) => format!("Google signed you in, but I couldn't keep it in the vault: {e}"),
                })
            }
            _ => None,
        }
    }

    // ------------------------------------------------------------ the tick

    pub(crate) fn social_tick(&mut self, t: u64, online: bool) {
        let cfg = self.social_cfg();
        if !cfg.enabled || !online || !(cfg.own_refresh || cfg.scan) {
            return;
        }
        // Once a minute is plenty to look.
        if t.saturating_sub(self.workday.social.last_look) < 60 {
            return;
        }
        self.workday.social.last_look = t;
        if cfg.own_refresh && t.saturating_sub(self.social_watch().last_refresh) >= cfg.refresh_hours.max(6) * 3600 {
            // unheard-ok: returns `String`, not a Result
            let _ = self.social_refresh(t, false);
        }
        if cfg.scan {
            // unheard-ok: returns `Option<String>`, not a Result
            let _ = self.social_scan(t, false);
        }
    }

    // ------------------------------------------------------------ the brief

    pub(crate) fn social_brief_items(&mut self, t: u64) -> Vec<crate::brief::Item> {
        use crate::brief::{Item, Outcome, Source, Weight};
        let cfg = self.social_cfg();
        if !cfg.enabled || !cfg.in_brief {
            return Vec::new();
        }
        let item = |id: &str, subject: String| Item {
            id: id.into(),
            source: Source::Day,
            from: "Social".into(),
            subject,
            weight: Weight::Info,
            outcome: Outcome::Yours,
            draft: None,
            conflicts_with: None,
        };
        let mut out = Vec::new();
        // Read only if there is a record at all.
        if self.store.root().join(super::snapshots::FILE).exists() {
            if let Some(line) = analysis::own_brief_line(self.social_book(), t) {
                out.push(item("social:own", line));
            }
        }
        if self.store.exists(watchlist::FILE) {
            let w = self.social_watch().clone();
            let fresh = w.list.iter().any(|x| x.last_ok.is_some_and(|ok| t.saturating_sub(ok) < 36 * 3600));
            if fresh {
                if let Some(line) = analysis::brief_digest(&w, t) {
                    out.push(item("social:watch", line));
                }
            }
        }
        out
    }

    // ------------------------------------------------------------ the hub page

    /// Disconnect YouTube: the sign-in out of the vault, and the permission
    /// taken back at Google when no Google calendar rides the same grant --
    /// Google ends a grant whole, every scope at once (N5).
    fn youtube_disconnect(&mut self, t: u64) -> String {
        if let Err(e) = self.vault_ready(t) {
            return format!("The vault is shut, so nothing changed: {e}");
        }
        let held = self.vault.get(VAULT_YOUTUBE_OAUTH, t).ok().and_then(|j| serde_json::from_str::<GoogleSignIn>(&j).ok());
        let Some(g) = held else {
            return "YouTube isn't connected through a sign-in, so there's nothing to take away.".into();
        };
        self.vault.secrets.retain(|s| s.name != VAULT_YOUTUBE_OAUTH);
        if let Err(e) = self.vault.save(&crate::roots::install_state()) {
            return format!("I couldn't save the vault, so YouTube is still connected: {e}");
        }
        crate::connect::forget_access(&format!("youtube {} {}", g.client_id, g.refresh_token));
        crate::connecting::note_google_data(self);
        let calendar_too = self
            .store
            .load::<Vec<crate::connect::CalendarLink>>(crate::connect::CALENDAR_LINKS)
            .iter()
            .any(|l| matches!(crate::oauthlink::parse_calendar_key(&l.url), Some((crate::oauthlink::Provider::Google, _))));
        if calendar_too && g.client_id == crate::oauthlink::GOOGLE_CLIENT_ID {
            return "YouTube is disconnected and its sign-in is gone from the vault. Google's permission stays, because your Google Calendar uses the same sign-in; disconnect that too on the Accounts page to end both.".into();
        }
        let token = g.refresh_token.clone();
        std::thread::spawn(move || {
            crate::heard!(crate::oauthlink::revoke_google(&Https, &token));
        });
        "YouTube is disconnected, its sign-in is gone from the vault, and Google's permission is being taken back too.".into()
    }

    /// Disconnect an Instagram, Threads, Facebook Page or TikTok sign-in: the
    /// secret out of the vault, the platform switched off so it isn't asked
    /// for again, and the access taken back at the provider where the provider
    /// has a call for it (TikTok does; Meta gives an app no way to end its own
    /// token, so its page is named instead) (N5).
    fn token_disconnect(&mut self, name: &str, t: u64) -> String {
        let (vault, label, setting, meta) = match name {
            "instagram" => (VAULT_INSTAGRAM, "Instagram", "instagram", true),
            "threads" => (VAULT_THREADS, "Threads", "threads", true),
            "facebook" => (VAULT_FACEBOOK, "Facebook Page", "facebook_page", true),
            "tiktok" => (VAULT_TIKTOK, "TikTok", "tiktok", false),
            _ => return "That isn't a sign-in I keep, so nothing changed.".into(),
        };
        if let Err(e) = self.vault_ready(t) {
            return format!("The vault is shut, so nothing changed: {e}");
        }
        let held = self.vault.get(vault, t).ok();
        let Some(held) = held else {
            return format!("{label} isn't connected through a kept sign-in, so there's nothing to take away.");
        };
        self.vault.secrets.retain(|s| s.name != vault);
        if let Err(e) = self.vault.save(&self.vault_home) {
            return format!("I couldn't save the vault, so {label} is still connected: {e}");
        }
        let _ = self.apply_setting(&format!("workday.social.{setting}"), "off"); // unheard-ok: the setting only stops the platform being read; the vault entry is already gone
        match name {
            "instagram" => self.social_watch().instagram_token_at = 0,
            "threads" => self.social_watch().threads_token_at = 0,
            _ => {}
        }
        crate::heard!(self.social_keep_watch());
        if meta {
            return format!(
                "{label} is disconnected and its token is gone from the vault. Meta gives an app like Atlas no way to end its own token, so it lapses on its own; to end it now, remove Atlas under Business Integrations in your Facebook settings ({}).",
                META_APPS
            );
        }
        match serde_json::from_str::<TikTokSignIn>(&held) {
            Ok(s) if !s.refresh_token.is_empty() => {
                std::thread::spawn(move || {
                    crate::heard!(apis::tiktok_access(&Https, &s).and_then(|(access, _)| apis::tiktok_revoke(&Https, &s, &access)));
                });
                "TikTok is disconnected, its sign-in is gone from the vault, and TikTok is being asked to end Atlas's access too.".into()
            }
            _ => "TikTok is disconnected and its sign-in is gone from the vault.".into(),
        }
    }

    /// "Connect my YouTube": the Connect button, said.
    pub(crate) fn connect_youtube(&mut self) -> String {
        match crate::oauthlink::google_secret() {
            Some(sec) => self.social_google(crate::oauthlink::GOOGLE_CLIENT_ID, &sec, crate::store::now()),
            None => crate::oauthlink::NO_GOOGLE_SECRET.into(),
        }
    }

    /// The Connect list: one row and one button per service (5 Oct 2026).
    fn social_services(&self, cfg: &SocialConfig, listed: &[String]) -> Vec<page::Service> {
        let has = |n: &str| listed.iter().any(|x| x == n);
        let mut out = Vec::new();
        let yt = has(VAULT_YOUTUBE_OAUTH) || has(VAULT_YOUTUBE_KEY);
        out.push(page::Service {
            name: "YouTube".into(),
            state: if yt { "Connected".into() } else { "Not connected".into() },
            connected: yt,
            button: crate::oauthlink::google_secret().is_some().then(|| ("google".to_string(), if yt { "Connect again".to_string() } else { "Connect YouTube".to_string() })),
            inner: String::new(),
            disconnect: has(VAULT_YOUTUBE_OAUTH).then(|| "youtube-disconnect".to_string()),
            note: if crate::oauthlink::google_secret().is_some() {
                "Your channel's numbers and retention, through your Google sign-in.".into()
            } else {
                "This copy of Atlas was built without its Google sign-in, so this waits for one that has it.".into()
            },
        });
        let handle = cfg.bluesky_handle.trim().trim_start_matches('@').to_string();
        out.push(page::Service {
            name: "Bluesky".into(),
            state: if handle.is_empty() { "Not connected".into() } else { format!("Connected as @{handle}") },
            connected: !handle.is_empty(),
            button: Some(("bluesky-handle".into(), if handle.is_empty() { "Connect Bluesky".into() } else { "Change".into() })),
            inner: format!("<label>Your handle <input name=handle value='{}' size=22 placeholder='you.bsky.social' required></label>", crate::hub::esc(&handle)),
            note: "Your public numbers need only your handle.".into(),
            disconnect: None,
        });
        out.push(page::Service {
            name: "Instagram, TikTok, X, Facebook, LinkedIn, Reddit".into(),
            state: "Through Atlas's own browser".into(),
            connected: false,
            button: Some(("browser-signin-all".into(), "Sign in to your accounts".into())),
            inner: String::new(),
            disconnect: None,
            note: "One window opens with a tab for each site: sign in on the ones you use, skip the rest, close it. \
                   Or just say \"sign me into my socials\"."
                .into(),
        });
        out
    }

    pub(crate) fn social_page(&mut self, said: Option<&str>) -> String {
        let t = crate::store::now();
        let cfg = self.social_cfg();
        let book = self.social_book().clone();
        let latest = book.latest_posts();
        let mut rows = Vec::new();
        for pf in book.platforms() {
            let days = book.account_days(pf);
            let f = days.iter().rev().find_map(|a| a.followers.map(|n| format!("{} on {}", super::count_said(n), super::day_label(a.day)))).unwrap_or_default();
            let mine: Vec<_> = latest.iter().filter(|p| p.platform == pf).collect();
            let last_day = mine.iter().map(|p| p.day).chain(days.iter().map(|a| a.day)).max();
            let source = mine.iter().max_by_key(|p| p.day).map(|p| p.source.clone()).or_else(|| days.last().map(|a| a.source.clone())).unwrap_or_default();
            rows.push(page::PlatformRow { name: pf.name().into(), posts: mine.len(), followers: f, latest: last_day.map(super::day_label).unwrap_or_default(), source });
        }
        let have = book.platforms();
        let nothing_from = Platform::ALL.iter().filter(|p| !have.contains(p)).map(|p| p.name().to_string()).collect();
        let min = self.tools_ref().map(|x| x.content.min_posts_for_patterns).unwrap_or(8);
        let w = self.social_watch().clone();
        crate::heard!(self.vault_ready(t));
        let vault_open = self.vault.state() == crate::vault::State::Open;
        let listed: Vec<String> = self.vault.list().iter().map(|(n, _)| n.to_string()).collect();
        let kept = |n: &str| vault_open.then(|| listed.iter().any(|x| x == n));
        let lapsing = if vault_open {
            self.vault.get(VAULT_YOUTUBE_OAUTH, t).ok().and_then(|j| serde_json::from_str::<GoogleSignIn>(&j).ok()).and_then(|g| g.lapsing(t, cfg.google_app_in_testing))
        } else {
            None
        };
        let mut notice: Vec<String> = said.map(|s| vec![s.to_string()]).unwrap_or_default();
        if !self.workday.social.last_missing.is_empty() {
            notice.push(format!("Last refresh couldn't read: {}.", self.workday.social.last_missing.join("; ")));
        }
        let services = self.social_services(&cfg, &listed);
        let v = page::View {
            services,
            notice: (!notice.is_empty()).then(|| notice.join(" ")),
            own_refresh: cfg.own_refresh,
            scan: cfg.scan,
            rows,
            nothing_from,
            last_video: if book.is_empty() { String::new() } else { analysis::last_video(&book, None, t) },
            worked: if book.is_empty() { String::new() } else { analysis::what_worked(&book, None, 30, t, min) },
            followers: if book.is_empty() { String::new() } else { analysis::followers(&book, None, t) },
            watching: w
                .list
                .iter()
                .map(|x| {
                    let how = if !x.last_error.is_empty() {
                        format!("last read failed: {}", x.last_error)
                    } else if let Some(ok) = x.last_ok {
                        format!("read {} minutes ago", t.saturating_sub(ok) / 60)
                    } else {
                        "not read yet".into()
                    };
                    (x.target.label(), how)
                })
                .collect(),
            digest: analysis::watch_digest(&w, t),
            summary: w.summary.as_ref().map(|(_, s)| s.clone()),
            keys: vec![
                ("YouTube API key".into(), kept(VAULT_YOUTUBE_KEY)),
                ("YouTube Analytics sign-in".into(), kept(VAULT_YOUTUBE_OAUTH)),
                ("Instagram token".into(), kept(VAULT_INSTAGRAM)),
                ("Threads token".into(), kept(VAULT_THREADS)),
                ("Facebook Page token".into(), kept(VAULT_FACEBOOK)),
                ("TikTok sign-in".into(), kept(VAULT_TIKTOK)),
                ("Bluesky app password (for posting)".into(), kept(VAULT_BLUESKY)),
            ],
            accounts: page::Accounts {
                youtube_channel: cfg.youtube_channel.clone(),
                bluesky_handle: cfg.bluesky_handle.clone(),
                instagram: cfg.instagram,
                threads: cfg.threads,
                facebook_page: cfg.facebook_page,
                tiktok: cfg.tiktok,
                youtube_analytics: cfg.youtube_analytics,
                google_app_in_testing: cfg.google_app_in_testing,
            },
            searches_left: w.quota.left(watchlist::pacific_day(t), cfg.youtube_searches_per_day),
            lapsing,
        };
        page::render_social(&v)
    }

    /// The Social page's buttons. What each did comes back on the page.
    pub(crate) fn social_post(&mut self, f: &[(String, String)], t: u64) -> String {
        let field = |k: &str| f.iter().find(|(n, _)| n == k).map(|(_, v)| v.trim().to_string()).unwrap_or_default();
        match field("what").as_str() {
            "import" => self.social_import(&field("path"), t),
            "watch" => self.social_watch_add(&format!("watch {}", field("target")), t),
            "browser-signin" => {
                let site = field("site");
                let Some((name, domain)) = super::SIGN_IN_SITES.iter().find(|(_, d)| *d == site) else {
                    return "That isn't one of the sites listed, so nothing was opened.".into();
                };
                let bcfg = self.tools_cfg().browser.clone();
                let vars = self.tools_cfg().vars.clone();
                match crate::browser::open_sign_in_window(&bcfg, &vars, &[crate::webrun::login_url(domain)]) {
                    Ok(()) => format!(
                        "{name} is open in Atlas's own browser window. Sign in there the way you normally do, codes included, then close the window. Atlas keeps that sign-in for reading your {name} pages."
                    ),
                    Err(e) => format!("Atlas's browser didn't open: {e}"),
                }
            }
            "browser-signin-all" => self.open_social_signins(),
            "bluesky-handle" => {
                let h = field("handle").trim().trim_start_matches('@').to_string();
                if h.is_empty() || h.contains(char::is_whitespace) || !h.contains('.') {
                    return "That doesn't look like a Bluesky handle -- it's the part after the @, like you.bsky.social.".into();
                }
                let mut said = self.apply_setting("workday.social.bluesky_handle", &h);
                if !self.social_cfg().own_refresh {
                    said.push(' ');
                    said.push_str(&self.apply_setting("workday.social.own_refresh", "on"));
                }
                said.push(' ');
                said.push_str(&self.social_refresh(t, true));
                said
            }
            "unwatch" => {
                let gone = self.social_watch().remove(&field("which"));
                crate::heard!(self.social_keep_watch());
                if gone.is_empty() { "Nothing matched, so nothing changed.".into() } else { format!("Stopped watching {}.", gone.join(", ")) }
            }
            "refresh" => self.social_refresh(t, true),
            "scan" => self.social_scan(t, true).unwrap_or_default(),
            "summarise" => {
                let w = self.social_watch().clone();
                let d = analysis::watch_digest(&w, t);
                if d.is_empty() {
                    "Nothing's been read to sum up yet.".into()
                } else {
                    self.social_summarise(t, &d, true).unwrap_or_else(|| "There's no model set up to sum it up; the figures are below.".into())
                }
            }
            "key" => {
                let (name, what) = match field("name").as_str() {
                    "youtube" => (VAULT_YOUTUBE_KEY, "YouTube API key"),
                    "instagram" => (VAULT_INSTAGRAM, "Instagram token"),
                    "threads" => (VAULT_THREADS, "Threads token"),
                    "facebook" => (VAULT_FACEBOOK, "Facebook Page token"),
                    "bluesky" => (VAULT_BLUESKY, "Bluesky app password"),
                    _ => return "That isn't a key I keep.".into(),
                };
                let secret = field("secret");
                if secret.is_empty() {
                    return "Nothing to keep.".into();
                }
                crate::heard!(self.vault_ready(t));
                match self.vault.put(name, crate::vault::Kind::ApiKey, &secret, t) {
                    Ok(()) => match self.vault.save(&self.vault_home) {
                        Ok(()) => {
                            if name == VAULT_INSTAGRAM {
                                self.social_watch().instagram_token_at = t;
                                crate::heard!(self.social_keep_watch());
                            }
                            if name == VAULT_THREADS {
                                self.social_watch().threads_token_at = t;
                                crate::heard!(self.social_keep_watch());
                            }
                            format!("Kept your {what} in the vault.")
                        }
                        Err(e) => format!("I couldn't save the vault: {e}"),
                    },
                    Err(e) => e,
                }
            }
            "google" => {
                let (id, secret) = (field("id"), field("secret"));
                // Atlas's own Google registration (`oauthlink`, published, so
                // the sign-in doesn't lapse weekly) unless you gave your own.
                if id.is_empty() && secret.is_empty() {
                    return match crate::oauthlink::google_secret() {
                        Some(sec) => self.social_google(crate::oauthlink::GOOGLE_CLIENT_ID, &sec, t),
                        None => crate::oauthlink::NO_GOOGLE_SECRET.into(),
                    };
                }
                if id.is_empty() || secret.is_empty() {
                    return "Both the client ID and its secret are needed (Google Cloud console, Credentials, a \"Desktop app\" client).".into();
                }
                self.social_google(&id, &secret, t)
            }
            "youtube-disconnect" => self.youtube_disconnect(t),
            "token-disconnect" => self.token_disconnect(&field("name"), t),
            "tiktok-start" => self.social_tiktok_start(&field("key"), &field("secret"), &field("redirect"), t),
            "tiktok-finish" => self.social_tiktok_finish(&field("address"), t),
            "accounts" => {
                // Each through Settings' own path, so what's kept is checked
                // the same way and lands in your preferences file.
                let mut said = Vec::new();
                for (key, v) in [("youtube_channel", field("youtube_channel")), ("bluesky_handle", field("bluesky_handle").trim_start_matches('@').to_string())] {
                    let full = format!("workday.social.{key}");
                    let now_is = if key == "youtube_channel" { self.social_cfg().youtube_channel } else { self.social_cfg().bluesky_handle };
                    if v != now_is {
                        said.push(self.apply_setting(&full, &v));
                    }
                }
                let cfg = self.social_cfg();
                for (key, now_on) in [
                    ("instagram", cfg.instagram),
                    ("threads", cfg.threads),
                    ("facebook_page", cfg.facebook_page),
                    ("tiktok", cfg.tiktok),
                    ("youtube_analytics", cfg.youtube_analytics),
                    ("google_app_in_testing", cfg.google_app_in_testing),
                ] {
                    let want = f.iter().any(|(n, v)| n == key && v == "on");
                    if want != now_on {
                        said.push(self.apply_setting(&format!("workday.social.{key}"), if want { "on" } else { "off" }));
                    }
                }
                if said.is_empty() { "Nothing changed.".into() } else { said.join(". ") }
            }
            _ => "That button isn't wired to anything, so nothing changed.".into(),
        }
    }

    /// TikTok's sign-in, first half: keep your app's details and open
    /// TikTok's consent page. TikTok sends you back to the address your app
    /// registered, which may be a page that doesn't load; you paste that
    /// address back (`social_tiktok_finish`).
    fn social_tiktok_start(&mut self, key: &str, secret: &str, redirect: &str, t: u64) -> String {
        if key.is_empty() || secret.is_empty() || !redirect.starts_with("https://") {
            return "The client key, the client secret and the https redirect address your TikTok app registered are all needed.".into();
        }
        if let Err(e) = self.vault_ready(t) {
            return format!("Nothing was started: {e}.");
        }
        let s = TikTokSignIn { client_key: key.into(), client_secret: secret.into(), redirect: redirect.into(), refresh_token: String::new(), state: crate::vault::short_code(16), obtained: 0 };
        let json = match serde_json::to_string(&s) {
            Ok(j) => j,
            Err(e) => return e.to_string(),
        };
        if let Err(e) = self.vault.put(VAULT_TIKTOK, crate::vault::Kind::ApiKey, &json, t) {
            return e;
        }
        crate::kept!(self.vault.save(&self.vault_home));
        match self.plat.open_path(&apis::tiktok_consent_url(&s)) {
            Ok(()) => "TikTok's sign-in is open in your browser. Say yes, then copy the address TikTok sends you to and paste it in the second box.".into(),
            Err(e) => format!("I couldn't open your browser for TikTok's sign-in: {e}"),
        }
    }

    /// TikTok's sign-in, second half: the code out of the address you pasted,
    /// swapped for tokens on the crew.
    fn social_tiktok_finish(&mut self, address: &str, t: u64) -> String {
        let s = match self.social_secret(VAULT_TIKTOK, t) {
            Ok(Some(j)) => match serde_json::from_str::<TikTokSignIn>(&j) {
                Ok(s) => s,
                Err(_) => return "Start the TikTok sign-in first (the first box).".into(),
            },
            Ok(None) => return "Start the TikTok sign-in first (the first box).".into(),
            Err(e) => return format!("I can't: {e}."),
        };
        let code = match apis::code_from_address(address, &s.state) {
            Ok(c) => c,
            Err(e) => return e,
        };
        let work: crate::crew::Work = Box::new(move |_ctl| {
            let (_, refresh) = apis::tiktok_exchange(&Https, &s, &code)?;
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            serde_json::to_string(&TikTokSignIn { refresh_token: refresh, state: String::new(), obtained: now, ..s }).map_err(|e| e.to_string())
        });
        match self.hand_off_social("social-tiktok", t, work, None, true) {
            Some(_) => "Finishing the TikTok sign-in; I'll say when it's done.".into(),
            None => "I've too much on to finish it right now. Try again in a minute.".into(),
        }
    }
}
