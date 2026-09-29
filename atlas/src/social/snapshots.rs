//! The record: one line per thing seen, per day, never rewritten.
//!
//! The platforms keep 28 to 90 days of analytics, or lifetime totals only.
//! So the history of a post -- views on day one, day seven, day thirty -- or
//! of a follower count exists only if something wrote it down each day, and
//! that is this file.
//!
//! **Append-only.** `social_snapshots.jsonl` in the state folder, one JSON
//! record a line. A crash mid-write loses at most the line being written; a
//! line that doesn't read is counted and skipped, never allowed to take the
//! rest with it. The same thing seen twice on one day with the same numbers
//! is not written twice; seen again with new numbers, the later line wins
//! for that day.
//!
//! **Nothing is filled in.** Every number is an `Option`: `None` is "this
//! platform, or this export, didn't say", which the answers name, and is
//! never shown as zero.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

pub const FILE: &str = "social_snapshots.jsonl";

/// The most lines read back. At a hundred posts a day for three years this
/// is not reached; past it the oldest are left on disk, unread, and said.
pub const MAX_LINES: usize = 400_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Youtube,
    Tiktok,
    Instagram,
    X,
    Linkedin,
    Bluesky,
    /// Meta's Threads (its own API, a token you make; 29 Sep 2026).
    Threads,
    /// A Facebook Page (Page Insights through a Page token). A personal
    /// profile has no insights API at all.
    Facebook,
}

impl Platform {
    pub const ALL: [Platform; 8] = [
        Platform::Youtube,
        Platform::Tiktok,
        Platform::Instagram,
        Platform::X,
        Platform::Linkedin,
        Platform::Bluesky,
        Platform::Threads,
        Platform::Facebook,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            Platform::Youtube => "YouTube",
            Platform::Tiktok => "TikTok",
            Platform::Instagram => "Instagram",
            Platform::X => "X",
            Platform::Linkedin => "LinkedIn",
            Platform::Bluesky => "Bluesky",
            Platform::Threads => "Threads",
            Platform::Facebook => "Facebook",
        }
    }

    /// Named in what you said: "on tiktok", "my youtube".
    pub fn in_words(said: &str) -> Option<Platform> {
        let low = said.to_lowercase();
        let has = |w: &str| low.split(|c: char| !c.is_alphanumeric()).any(|x| x == w);
        if has("youtube") || has("yt") || has("shorts") {
            Some(Platform::Youtube)
        } else if has("tiktok") || has("tik") {
            Some(Platform::Tiktok)
        } else if has("instagram") || has("insta") || has("ig") || has("reel") || has("reels") {
            Some(Platform::Instagram)
        } else if has("twitter") || has("x") || has("tweets") {
            Some(Platform::X)
        } else if has("linkedin") {
            Some(Platform::Linkedin)
        } else if has("bluesky") || has("bsky") {
            Some(Platform::Bluesky)
        } else if has("threads") {
            Some(Platform::Threads)
        } else if has("facebook") || has("fb") {
            Some(Platform::Facebook)
        } else {
            None
        }
    }

    /// Where the posts are videos, so "my last video" means something.
    pub fn is_video(&self) -> bool {
        matches!(self, Platform::Youtube | Platform::Tiktok | Platform::Instagram)
    }
}

/// The numbers, each one there only if the source said it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Metrics {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub views: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub likes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comments: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shares: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saves: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reposts: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quotes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub impressions: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reach: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engagements: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watch_minutes: Option<f64>,
    /// Average seconds watched per view.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_view_secs: Option<f64>,
    /// Average share of the video watched, 0 to 100 -- the one retention
    /// figure that is free (YouTube Analytics, a Studio export).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_view_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub followers_gained: Option<i64>,
}

impl Metrics {
    /// Likes, comments, shares, saves, reposts and quotes that were given,
    /// summed; `None` when not one of them was.
    pub fn interactions(&self) -> Option<u64> {
        let parts = [self.likes, self.comments, self.shares, self.saves, self.reposts, self.quotes];
        if parts.iter().all(|p| p.is_none()) {
            return self.engagements;
        }
        Some(parts.iter().flatten().sum())
    }

    /// Fill what this lacks from `other` (a second source for the same post
    /// on the same day: the Data API's views and the Analytics API's
    /// retention). What this already has is kept.
    pub fn fill_from(&mut self, other: &Metrics) {
        macro_rules! take {
            ($($f:ident),*) => { $( if self.$f.is_none() { self.$f = other.$f; } )* };
        }
        take!(views, likes, comments, shares, saves, reposts, quotes, impressions, reach, engagements, watch_minutes, avg_view_secs, avg_view_pct, followers_gained);
    }
}

/// A post, as seen on one day.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PostSnap {
    pub platform: Platform,
    pub id: String,
    /// The day these numbers are as of (days since 1970, UTC).
    pub day: i64,
    /// When it was written down.
    pub taken: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub posted: Option<u64>,
    /// The title or caption (its first 280 characters).
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seconds: Option<f64>,
    /// Where the numbers came from, said plainly: "your X archive",
    /// "the YouTube Data API".
    pub source: String,
    #[serde(default)]
    pub m: Metrics,
}

/// An account, as seen on one day.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountSnap {
    pub platform: Platform,
    #[serde(default)]
    pub handle: String,
    pub day: i64,
    pub taken: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub followers: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub following: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub posts: Option<u64>,
    pub source: String,
    /// The account's own figures for that day, where a platform gives them
    /// (LinkedIn's daily impressions, say).
    #[serde(default)]
    pub m: Metrics,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Record {
    Post(PostSnap),
    Account(AccountSnap),
}

impl Record {
    fn key(&self) -> (u8, Platform, String, i64) {
        match self {
            Record::Post(p) => (0, p.platform, p.id.clone(), p.day),
            Record::Account(a) => (1, a.platform, a.handle.to_lowercase(), a.day),
        }
    }

    /// The same numbers, whenever and from wherever it was taken.
    fn same_numbers(&self, other: &Record) -> bool {
        match (self, other) {
            (Record::Post(a), Record::Post(b)) => a.m == b.m && a.text == b.text && a.seconds == b.seconds && a.posted == b.posted,
            (Record::Account(a), Record::Account(b)) => a.followers == b.followers && a.following == b.following && a.posts == b.posts && a.m == b.m,
            _ => false,
        }
    }
}

/// Everything written down, read back: the last word per thing per day.
#[derive(Debug, Clone, Default)]
pub struct Book {
    by_key: BTreeMap<(u8, Platform, String, i64), Record>,
    /// Lines that didn't read (a torn last line, a hand edit).
    pub unreadable: usize,
    /// Lines left unread past `MAX_LINES`.
    pub left_unread: usize,
}

/// What an `add` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Added {
    pub new: usize,
    pub unchanged: usize,
}

impl Book {
    pub fn load(path: &Path) -> Book {
        let mut b = Book::default();
        let Ok(text) = std::fs::read_to_string(path) else { return b };
        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        let skip = lines.len().saturating_sub(MAX_LINES);
        b.left_unread = skip;
        for line in &lines[skip..] {
            match serde_json::from_str::<Record>(line) {
                Ok(r) => {
                    b.by_key.insert(r.key(), r);
                }
                Err(_) => b.unreadable += 1,
            }
        }
        b
    }

    /// Write what's new to the end of the file, and take it in here. A
    /// record whose numbers match what's already kept for that day is not
    /// written again.
    pub fn add(&mut self, path: &Path, records: Vec<Record>) -> std::io::Result<Added> {
        let mut out = String::new();
        let mut added = Added::default();
        let mut fresh: Vec<Record> = Vec::new();
        for r in records {
            let k = r.key();
            if self.by_key.get(&k).is_some_and(|old| old.same_numbers(&r)) || fresh.iter().any(|f| f.key() == k && f.same_numbers(&r)) {
                added.unchanged += 1;
                continue;
            }
            out.push_str(&serde_json::to_string(&r).map_err(std::io::Error::other)?);
            out.push('\n');
            added.new += 1;
            fresh.push(r);
        }
        if !out.is_empty() {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
            f.write_all(out.as_bytes())?;
            f.sync_all()?;
        }
        for r in fresh {
            self.by_key.insert(r.key(), r);
        }
        Ok(added)
    }

    /// Each post's most recent snapshot.
    pub fn latest_posts(&self) -> Vec<&PostSnap> {
        let mut last: BTreeMap<(Platform, &str), &PostSnap> = BTreeMap::new();
        for r in self.by_key.values() {
            if let Record::Post(p) = r {
                // Keys sort by day, so the later one overwrites.
                last.insert((p.platform, p.id.as_str()), p);
            }
        }
        last.into_values().collect()
    }

    /// One post's snapshots, oldest day first.
    pub fn history(&self, platform: Platform, id: &str) -> Vec<&PostSnap> {
        self.by_key
            .values()
            .filter_map(|r| match r {
                Record::Post(p) if p.platform == platform && p.id == id => Some(p),
                _ => None,
            })
            .collect()
    }

    /// An account's snapshots on one platform, oldest day first.
    pub fn account_days(&self, platform: Platform) -> Vec<&AccountSnap> {
        let mut v: Vec<&AccountSnap> = self
            .by_key
            .values()
            .filter_map(|r| match r {
                Record::Account(a) if a.platform == platform => Some(a),
                _ => None,
            })
            .collect();
        v.sort_by_key(|a| a.day);
        v
    }

    /// Platforms something has been kept for.
    pub fn platforms(&self) -> Vec<Platform> {
        let mut v: Vec<Platform> = self.by_key.keys().map(|k| k.1).collect();
        v.dedup();
        v.sort();
        v.dedup();
        v
    }

    /// How many records are kept, after one-per-day.
    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }
}
