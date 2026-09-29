//! TikTok, Instagram and X: one page, when you ask, and never on a schedule.
//!
//! None of the three has a free legitimate way to follow other people's
//! posts. Their terms forbid automated collection (TikTok: "scrape, crawl
//! ... using any automated system"; X: scraping "in any form, for any
//! purpose", with damages per million posts; Instagram likewise), and Atlas's
//! browser is signed in as you, so a crawler would be spending *your*
//! account -- rate walls, checkpoints, suspension of the account you're
//! trying to grow. Meta v. Bright Data (2024) turned on logged-*off* reading;
//! signed-in automated collection is exactly what those terms govern.
//!
//! What Atlas does instead is what you'd do: open the page you name, once,
//! in its own browser, read what's on the screen, and close it. At most one
//! page a site every two minutes, and only from something you said.

use std::collections::HashMap;

/// Why these three are never on the watch list's schedule.
pub const WHY_NO_SCHEDULE: &str = "TikTok, Instagram and X give no free way to follow other people's posts, their terms forbid automated collection, and Atlas's browser is signed in as you -- so a schedule would put your own account at risk. What I can do: open one page when you ask (\"read this tiktok\" with its address) and tell you what's on it.";

/// Between two pages from one site.
pub const SPACING_SECS: u64 = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Site {
    Tiktok,
    Instagram,
    X,
}

impl Site {
    pub fn name(&self) -> &'static str {
        match self {
            Site::Tiktok => "TikTok",
            Site::Instagram => "Instagram",
            Site::X => "X",
        }
    }
}

/// Only an https page on one of the three sites.
pub fn check_url(url: &str) -> Result<(Site, String), String> {
    let u = url.trim().trim_end_matches(['.', ',', ')']);
    let rest = u.strip_prefix("https://").ok_or("Give me the page's full address, starting https://")?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or("").to_lowercase();
    let bare = host.trim_start_matches("www.").trim_start_matches("m.").trim_start_matches("mobile.");
    let site = match bare {
        "tiktok.com" | "vm.tiktok.com" => Site::Tiktok,
        "instagram.com" => Site::Instagram,
        "x.com" | "twitter.com" => Site::X,
        _ => return Err("I only open TikTok, Instagram or X pages this way -- anything else, ask me to read it and I'll use the ordinary reader.".into()),
    };
    Ok((site, u.to_string()))
}

/// The first https address in what you said.
pub fn url_in(said: &str) -> Option<String> {
    said.split_whitespace().find(|w| w.starts_with("https://")).map(|w| w.trim_end_matches(['.', ',', ')']).to_string())
}

/// One page a site every two minutes.
#[derive(Debug, Clone, Default)]
pub struct Spacing {
    last: HashMap<Site, u64>,
}

impl Spacing {
    /// Ok to go now, or the seconds to wait.
    pub fn may(&mut self, site: Site, now: u64) -> Result<(), u64> {
        if let Some(t) = self.last.get(&site) {
            let since = now.saturating_sub(*t);
            if since < SPACING_SECS {
                return Err(SPACING_SECS - since);
            }
        }
        self.last.insert(site, now);
        Ok(())
    }
}

/// A figure as the page shows it: "1.2M views", "12.3K Likes", "345
/// comments". Read from the page's words, labelled as shown -- rounded by the
/// site, not by Atlas.
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    pub what: String,
    pub value: u64,
    pub as_shown: String,
}

fn shown_number(tok: &str) -> Option<u64> {
    let t = tok.trim().trim_end_matches(['.', ',', ':']).replace(',', "");
    let (num, mult) = match t.chars().last()? {
        'K' | 'k' => (&t[..t.len() - 1], 1_000.0),
        'M' | 'm' => (&t[..t.len() - 1], 1_000_000.0),
        'B' | 'b' => (&t[..t.len() - 1], 1_000_000_000.0),
        _ => (t.as_str(), 1.0),
    };
    let v: f64 = num.parse().ok()?;
    (v >= 0.0 && v.is_finite()).then_some((v * mult).round() as u64)
}

const WORDS: &[&str] = &["views", "likes", "comments", "followers", "following", "reposts", "shares", "saves", "bookmarks", "plays", "replies", "quotes"];

/// The counts written on a page: a number beside one of the words above.
pub fn counts_on_page(text: &str) -> Vec<Shown> {
    let toks: Vec<&str> = text.split_whitespace().collect();
    let mut out: Vec<Shown> = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        let w = t.trim_matches(|c: char| !c.is_alphabetic()).to_lowercase();
        if !WORDS.contains(&w.as_str()) || out.iter().any(|s| s.what == w) {
            continue;
        }
        // "1.2M views" or "Followers 12K".
        let before = i.checked_sub(1).and_then(|j| toks.get(j)).and_then(|x| shown_number(x).map(|v| (v, *x)));
        let after = toks.get(i + 1).and_then(|x| shown_number(x).map(|v| (v, *x)));
        if let Some((v, raw)) = before.or(after) {
            out.push(Shown { what: w, value: v, as_shown: format!("{raw} {}", t.trim_matches(|c: char| !c.is_alphabetic())) });
        }
    }
    out
}

/// What's said about a page once it's been read.
pub fn said_about(site: Site, url: &str, text: &str) -> String {
    if text.trim().is_empty() {
        return format!("{} showed nothing readable at that address -- it may want you signed in, or it blocked the visit.", site.name());
    }
    let counts = counts_on_page(text);
    let mut s = format!("From the {} page (read once, as shown on screen): ", site.name());
    if counts.is_empty() {
        s.push_str("no counts I could read on it.");
    } else {
        s.push_str(&counts.iter().map(|c| c.as_shown.clone()).collect::<Vec<_>>().join(", "));
        s.push_str(". Those are the site's own rounded figures.");
    }
    let first: String = text.split_whitespace().take(60).collect::<Vec<_>>().join(" ");
    s.push_str(&format!(" It opens: \"{first}\" ({url})"));
    s
}
