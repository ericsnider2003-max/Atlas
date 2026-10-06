//! Following sites without visiting them: RSS and Atom feeds, read here, new
//! items listed, and the ones you want kept to read later.
//!
//! "Follow theverge.com." "What's new?" "Read 2." "Save 3 for later."
//!
//! **Sources:** Miniflux (Apache-2.0) read for what a feed reader must get
//! right -- RSS 2.0, RSS 1.0 (RDF) and Atom all in the wild; ids that are
//! sometimes missing (fall back to the link); feeds found from a page's
//! `<link rel="alternate">`; backing off from a feed that keeps failing; and
//! stripping tracking parameters from links (its rewrite rules, and the
//! ClearURLs rule list, read for which parameters). The XML reader here is
//! new and deliberately small.
//!
//! **Soundproofing.**
//! - A feed's first read marks everything seen and lists only its newest
//!   three, so following a site never floods you with its archive.
//! - A feed that fails backs off -- the interval doubles per failure, to a
//!   day -- and says so in "what's new", rather than retrying every tick.
//! - Bounded: 200 feeds, 500 remembered ids each, 300 unread, 4 MB a feed
//!   (the http reader's own cap), XML depth 64.
//! - Only http(s) links are kept; `javascript:` and `data:` never are.
//! - Redirects are followed three hops at most, and never from https down
//!   to http.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub const MAX_FEEDS: usize = 200;
pub const MAX_SEEN: usize = 500;
pub const MAX_UNREAD: usize = 300;
const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Item {
    pub id: String,
    pub title: String,
    pub link: String,
    pub published: Option<u64>,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Parsed {
    pub title: String,
    pub items: Vec<Item>,
}

// ---- XML, small ------------------------------------------------------------

/// Decode the five named entities and numeric references. Anything else is
/// left as written (HTML entities in a summary are for the page, not us).
fn unescape_xml(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        // At most 12 bytes on, cut back to a character boundary: a bullet
        // "•" straddling byte 12 panicked here and stopped Atlas (30 Sep
        // 2026, a job feed).
        let mut lim = tail.len().min(12);
        while !tail.is_char_boundary(lim) {
            lim -= 1;
        }
        let Some(semi) = tail[..lim].find(';') else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let name = &tail[1..semi];
        let ch = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            n if n.starts_with("#x") || n.starts_with("#X") => u32::from_str_radix(&n[2..], 16).ok().and_then(char::from_u32),
            n if n.starts_with('#') => n[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &tail[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let low = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = low[from..].find(name) {
        let at = from + i;
        from = at + name.len();
        let before_ok = at == 0 || low.as_bytes()[at - 1].is_ascii_whitespace();
        let after = low[at + name.len()..].trim_start();
        if !before_ok || !after.starts_with('=') {
            continue;
        }
        let v = tag[tag.len() - after.len() + 1..].trim_start();
        let q = v.chars().next()?;
        return if q == '"' || q == '\'' {
            let end = v[1..].find(q)?;
            Some(unescape_xml(&v[1..1 + end]))
        } else {
            Some(unescape_xml(v.split(|c: char| c.is_whitespace() || c == '/' || c == '>').next().unwrap_or("")))
        };
    }
    None
}

/// The local part of a tag name, lower-cased: `atom:link` -> `link`.
fn local(name: &str) -> String {
    name.rsplit(':').next().unwrap_or(name).to_ascii_lowercase()
}

/// Strip tags from a summary, leaving its words.
pub fn text_of(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    unescape_xml(&out).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Read a feed: RSS 2.0, RSS 1.0 (RDF) or Atom.
pub fn parse(xml: &str) -> Result<Parsed, String> {
    let mut p = Parsed::default();
    let mut stack: Vec<String> = Vec::new();
    let mut cur: Option<Item> = None;
    let mut text = String::new();
    let mut i = 0;
    let b = xml.as_bytes();
    let mut saw_root = false;
    while i < b.len() {
        if b[i] != b'<' {
            let end = xml[i..].find('<').map(|n| i + n).unwrap_or(b.len());
            text.push_str(&unescape_xml(&xml[i..end]));
            i = end;
            continue;
        }
        let rest = &xml[i..];
        if rest.starts_with("<![CDATA[") {
            let end = rest.find("]]>").ok_or("a CDATA section never closes")?;
            text.push_str(&rest[9..end]);
            i += end + 3;
            continue;
        }
        if rest.starts_with("<!--") {
            i += rest.find("-->").map(|n| n + 3).ok_or("a comment never closes")?;
            continue;
        }
        if rest.starts_with("<?") || rest.starts_with("<!") {
            i += rest.find('>').map(|n| n + 1).ok_or("a declaration never closes")?;
            continue;
        }
        let end = rest.find('>').ok_or("a tag never closes")?;
        let tag = &rest[1..end];
        i += end + 1;
        if let Some(name) = tag.strip_prefix('/') {
            let name = local(name.trim());
            // Close to the matching open; a stray close is ignored.
            if let Some(pos) = stack.iter().rposition(|s| *s == name) {
                let t = text.trim().to_string();
                let parent = if pos > 0 { stack[pos - 1].as_str() } else { "" };
                if let Some(it) = cur.as_mut() {
                    match name.as_str() {
                        "title" if parent == "item" || parent == "entry" => it.title = text_of(&t),
                        "link" if (parent == "item") && it.link.is_empty() => it.link = t.clone(),
                        "guid" | "id" if parent == "item" || parent == "entry" => it.id = t.clone(),
                        "pubdate" | "date" | "published" | "updated" | "issued" if it.published.is_none() || name == "published" => {
                            it.published = parse_date(&t).or(it.published)
                        }
                        "description" | "summary" | "content" | "encoded" if it.summary.is_empty() || name == "summary" || name == "description" => {
                            it.summary = text_of(&t).chars().take(400).collect()
                        }
                        _ => {}
                    }
                } else if name == "title" && p.title.is_empty() && (parent == "channel" || parent == "feed") {
                    p.title = text_of(&t);
                }
                if name == "item" || name == "entry" {
                    if let Some(mut it) = cur.take() {
                        it.link = clean_link(&it.link).unwrap_or_default();
                        if it.id.is_empty() {
                            it.id = if it.link.is_empty() { it.title.clone() } else { it.link.clone() };
                        }
                        if !it.id.is_empty() {
                            p.items.push(it);
                        }
                    }
                }
                stack.truncate(pos);
            }
            text.clear();
            continue;
        }
        let self_closing = tag.ends_with('/');
        let name_raw = tag.trim_end_matches('/').split_whitespace().next().unwrap_or("");
        let name = local(name_raw);
        if !saw_root {
            if !matches!(name.as_str(), "rss" | "feed" | "rdf") {
                return Err(format!("that isn't a feed (it starts with <{name}>)"));
            }
            saw_root = true;
        }
        if name == "item" || name == "entry" {
            cur = Some(Item::default());
        }
        // Atom's link is an attribute: the alternate (or unmarked) one.
        if name == "link" {
            if let (Some(it), Some(href)) = (cur.as_mut(), attr(tag, "href")) {
                let rel = attr(tag, "rel").unwrap_or_else(|| "alternate".into());
                if rel == "alternate" && it.link.is_empty() {
                    it.link = href;
                }
            }
        }
        if name == "item" {
            if let (Some(it), Some(about)) = (cur.as_mut(), attr(tag, "rdf:about")) {
                it.id = about;
            }
        }
        if !self_closing {
            stack.push(name);
            if stack.len() > MAX_DEPTH {
                return Err("nested too deep to be a feed".into());
            }
        }
        text.clear();
    }
    if !saw_root {
        return Err("that's empty, not a feed".into());
    }
    Ok(p)
}

/// RFC 822 (RSS) or RFC 3339 (Atom) to UTC seconds.
pub fn parse_date(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.len() >= 10 && s.as_bytes()[4] == b'-' {
        let y: i64 = s.get(0..4)?.parse().ok()?;
        let m: u32 = s.get(5..7)?.parse().ok()?;
        let d: u32 = s.get(8..10)?.parse().ok()?;
        let (mut hh, mut mm, mut ss, mut off) = (0i64, 0i64, 0i64, 0i64);
        if s.len() >= 19 {
            hh = s.get(11..13)?.parse().ok()?;
            mm = s.get(14..16)?.parse().ok()?;
            ss = s.get(17..19)?.parse().ok()?;
            let tz = s.get(19..)?.trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
            if tz.len() >= 6 && (tz.starts_with('+') || tz.starts_with('-')) {
                let sign = if tz.starts_with('-') { -1 } else { 1 };
                let oh: i64 = tz.get(1..3)?.parse().ok()?;
                let om: i64 = tz.get(4..6)?.parse().ok()?;
                off = sign * (oh * 3600 + om * 60);
            }
        }
        if !(1..=12).contains(&m) || d == 0 || d > 31 || !(0..24).contains(&hh) || !(0..60).contains(&mm) || !(0..=60).contains(&ss) {
            return None;
        }
        let t = crate::civil::days_from_civil(y, m, d) * 86_400 + hh * 3600 + mm * 60 + ss - off;
        return u64::try_from(t).ok();
    }
    crate::triage::parse_rfc2822(s)
}

// ---- links -----------------------------------------------------------------

/// Query parameters that only follow you around.
const TRACKERS: &[&str] = &[
    "fbclid", "gclid", "dclid", "gbraid", "wbraid", "msclkid", "yclid", "mc_cid", "mc_eid", "igshid", "_hsenc", "_hsmi",
    "mkt_tok", "oly_anon_id", "oly_enc_id", "vero_id", "vero_conv", "ref_src", "ref_url", "__s", "s_cid", "icid",
    "cmpid", "ncid", "sr_share", "ocid", "twclid", "ttclid", "rb_clickid", "wickedid", "_openstat",
];

/// A link with its tracking parameters gone; `None` for anything that isn't
/// http(s).
pub fn clean_link(url: &str) -> Option<String> {
    let url = url.trim();
    let low = url.to_ascii_lowercase();
    if !(low.starts_with("https://") || low.starts_with("http://")) {
        return None;
    }
    let (before_frag, frag) = match url.split_once('#') {
        Some((a, f)) => (a, Some(f)),
        None => (url, None),
    };
    let (base, query) = before_frag.split_once('?').unwrap_or((before_frag, ""));
    let kept: Vec<&str> = query
        .split('&')
        .filter(|kv| !kv.is_empty())
        .filter(|kv| {
            let k = kv.split('=').next().unwrap_or("").to_ascii_lowercase();
            !k.is_empty() && !k.starts_with("utm_") && !TRACKERS.contains(&k.as_str())
        })
        .collect();
    let mut out = base.to_string();
    if !kept.is_empty() {
        out.push('?');
        out.push_str(&kept.join("&"));
    }
    if let Some(f) = frag {
        // "#xtor=RSS-3" and the like are trackers too; a real anchor stays.
        if !f.contains('=') && !f.is_empty() {
            out.push('#');
            out.push_str(f);
        }
    }
    Some(out)
}

/// (scheme is https, host, path+query).
pub fn split_feed_url(url: &str) -> Option<(bool, String, String)> {
    let low = url.to_ascii_lowercase();
    let (https, rest) = if low.starts_with("https://") {
        (true, &url[8..])
    } else if low.starts_with("http://") {
        (false, &url[7..])
    } else {
        return None;
    };
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let host = host.split('@').next_back().unwrap_or(host);
    if host.is_empty() || host.contains(|c: char| c.is_whitespace()) {
        return None;
    }
    Some((https, host.to_ascii_lowercase(), path.split('#').next().unwrap_or("/").to_string()))
}

/// A link relative to a page, made absolute.
pub fn absolute_link(base: &str, href: &str) -> Option<String> {
    let href = href.trim();
    if href.starts_with("http://") || href.starts_with("https://") {
        return Some(href.to_string());
    }
    let (https, host, path) = split_feed_url(base)?;
    let scheme = if https { "https" } else { "http" };
    if let Some(rest) = href.strip_prefix("//") {
        return Some(format!("{scheme}://{rest}"));
    }
    if href.starts_with('/') {
        return Some(format!("{scheme}://{host}{href}"));
    }
    let dir = path.split('?').next().unwrap_or("/");
    let dir = &dir[..dir.rfind('/').map(|i| i + 1).unwrap_or(1)];
    Some(format!("{scheme}://{host}{dir}{href}"))
}

/// The feeds a web page points at (`<link rel="alternate" type="…rss…">`).
pub fn discover(html: &str, page: &str) -> Vec<String> {
    let mut out = Vec::new();
    let low = html.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = low[from..].find("<link") {
        let at = from + i;
        let end = low[at..].find('>').map(|n| at + n).unwrap_or(low.len());
        let tag = &html[at + 5..end];
        from = end;
        let ty = attr(tag, "type").unwrap_or_default().to_ascii_lowercase();
        let rel = attr(tag, "rel").unwrap_or_default().to_ascii_lowercase();
        if rel.split_whitespace().any(|r| r == "alternate") && (ty.contains("rss") || ty.contains("atom")) {
            if let Some(href) = attr(tag, "href").and_then(|h| absolute_link(page, &h)) {
                if !out.contains(&href) {
                    out.push(href);
                }
            }
        }
    }
    out
}

/// Fetch a URL, following up to three redirects and never from https down
/// to http. `get(https, host, path)` does the request.
pub fn fetch(url: &str, get: &dyn Fn(bool, &str, &str) -> crate::error::Result<crate::http::Response>) -> Result<(String, String), String> {
    let mut url = url.to_string();
    let mut was_https = false;
    for _ in 0..4 {
        let (https, host, path) = split_feed_url(&url).ok_or_else(|| format!("{url} isn't a web address"))?;
        if was_https && !https {
            return Err(format!("{url}: it redirects from https to plain http, which I won't follow"));
        }
        was_https |= https;
        let r = get(https, &host, &path).map_err(|e| e.to_string())?;
        if (300..400).contains(&r.status) {
            let loc = r.location.ok_or_else(|| format!("{url}: a redirect with nowhere to go"))?;
            url = absolute_link(&url, &loc).ok_or_else(|| format!("{url}: a redirect to {loc}"))?;
            continue;
        }
        if !r.ok() {
            return Err(format!("{host} answered {}", r.status));
        }
        return Ok((url, r.body));
    }
    Err(format!("{url}: too many redirects"))
}

// ---- what you follow -------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Feed {
    pub url: String,
    pub title: String,
    #[serde(default)]
    pub seen: VecDeque<String>,
    #[serde(default)]
    pub next_due: u64,
    #[serde(default)]
    pub failures: u32,
    #[serde(default)]
    pub last_error: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Unread {
    pub feed: String,
    pub title: String,
    pub link: String,
    pub at: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Feeds {
    pub feeds: Vec<Feed>,
    #[serde(default)]
    pub unread: Vec<Unread>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FeedsConfig {
    pub enabled: bool,
    /// Minutes between reads of one feed.
    pub every_minutes: u64,
}

impl Default for FeedsConfig {
    fn default() -> Self {
        FeedsConfig { enabled: true, every_minutes: 120 }
    }
}

impl Feeds {
    pub fn follow(&mut self, url: &str, title: &str) -> Result<bool, String> {
        let url = clean_link(url).ok_or("that isn't a web address")?;
        if self.feeds.iter().any(|f| f.url == url) {
            return Ok(false);
        }
        if self.feeds.len() >= MAX_FEEDS {
            return Err(format!("you're following {MAX_FEEDS} feeds already"));
        }
        self.feeds.push(Feed { url, title: title.to_string(), ..Default::default() });
        Ok(true)
    }

    /// Stop following: by number (1-based), title or address fragment.
    pub fn unfollow(&mut self, which: &str) -> Option<String> {
        let w = which.trim().to_lowercase();
        let i = w
            .parse::<usize>()
            .ok()
            .and_then(|n| n.checked_sub(1))
            .filter(|i| *i < self.feeds.len())
            .or_else(|| self.feeds.iter().position(|f| f.title.to_lowercase() == w))
            .or_else(|| {
                let hits: Vec<usize> = (0..self.feeds.len()).filter(|i| self.feeds[*i].url.to_lowercase().contains(&w) || self.feeds[*i].title.to_lowercase().contains(&w)).collect();
                (hits.len() == 1).then(|| hits[0])
            })?;
        let f = self.feeds.remove(i);
        self.unread.retain(|u| u.feed != f.url);
        Some(if f.title.is_empty() { f.url } else { f.title })
    }

    /// Feeds whose turn it is.
    pub fn due(&self, now: u64) -> Vec<usize> {
        (0..self.feeds.len()).filter(|i| self.feeds[*i].next_due <= now).collect()
    }

    /// A read came back: add what's new. Returns how many were new.
    pub fn took(&mut self, i: usize, parsed: Parsed, cfg: &FeedsConfig, now: u64) -> usize {
        let Some(f) = self.feeds.get_mut(i) else { return 0 };
        let first = f.seen.is_empty();
        if f.title.is_empty() {
            f.title = parsed.title.chars().take(80).collect();
        }
        f.failures = 0;
        f.last_error.clear();
        f.next_due = now + cfg.every_minutes.max(15) * 60;
        let mut fresh: Vec<Item> = parsed.items.into_iter().filter(|it| !f.seen.contains(&it.id)).collect();
        for it in &fresh {
            f.seen.push_back(it.id.clone());
        }
        while f.seen.len() > MAX_SEEN {
            f.seen.pop_front();
        }
        // Newest first; undated ones keep their order after the dated.
        fresh.sort_by_key(|b| std::cmp::Reverse(b.published));
        if first {
            fresh.truncate(3);
        }
        let name = if f.title.is_empty() { f.url.clone() } else { f.title.clone() };
        let url = f.url.clone();
        let n = fresh.len();
        for it in fresh {
            if it.link.is_empty() || self.unread.iter().any(|u| u.link == it.link) {
                continue;
            }
            self.unread.push(Unread { feed: url.clone(), title: format!("{} — {}", it.title, name), link: it.link, at: it.published.unwrap_or(now) });
        }
        self.unread.sort_by_key(|b| std::cmp::Reverse(b.at));
        self.unread.truncate(MAX_UNREAD);
        n
    }

    /// A read failed: back off, doubling to a day.
    pub fn failed(&mut self, i: usize, why: &str, cfg: &FeedsConfig, now: u64) {
        if let Some(f) = self.feeds.get_mut(i) {
            f.failures = f.failures.saturating_add(1);
            f.last_error = why.chars().take(120).collect();
            let wait = (cfg.every_minutes.max(15) * 60).saturating_mul(1 << f.failures.min(10)).min(86_400);
            f.next_due = now + wait;
        }
    }

    /// "What's new": numbered, newest first, with any feed that's failing.
    pub fn said(&self, n: usize) -> String {
        let mut out: Vec<String> = self.unread.iter().take(n).enumerate().map(|(i, u)| format!("{}. {}", i + 1, u.title)).collect();
        if out.is_empty() {
            out.push(if self.feeds.is_empty() { "You're not following anything. \"Follow <site>\" starts one.".into() } else { "Nothing new.".into() });
        } else if self.unread.len() > n {
            out.push(format!("(and {} more)", self.unread.len() - n));
        }
        for f in self.feeds.iter().filter(|f| f.failures >= 3) {
            out.push(format!("{} has failed {} times running: {}", if f.title.is_empty() { &f.url } else { &f.title }, f.failures, f.last_error));
        }
        out.join("\n")
    }

    /// Take item `n` (1-based) off the unread list.
    pub fn take(&mut self, n: usize) -> Option<Unread> {
        let i = n.checked_sub(1)?;
        (i < self.unread.len()).then(|| self.unread.remove(i))
    }
}
