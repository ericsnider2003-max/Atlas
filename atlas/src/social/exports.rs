//! The platforms' own export files, read into snapshots.
//!
//! Each platform will hand you your data for free -- that is the one route
//! every one of them has. What each file holds, and where the shapes here
//! come from (**checked only against the public descriptions below, and
//! against synthetic files built to match them in `tests/fixtures/social`;
//! not against a real export of Eric's, which none of these has seen**):
//!
//! - **X** (Settings > Your account > Download an archive): a zip with
//!   `data/tweets.js` (older archives: `data/tweet.js`, and big ones split
//!   into `tweets-part1.js`...), each `window.YTD.tweets.part0 = [...]` with
//!   `{"tweet": {"id_str", "full_text", "created_at": "Tue Sep 24 14:03:11
//!   +0000 2024", "favorite_count", "retweet_count"}}`; `data/account.js`
//!   (`username`); `data/follower.js` (one entry per follower);
//!   `data/manifest.js` (`generationDate`). No impressions: those need
//!   Premium. Source: X Help "How to download your X archive" and the
//!   archive's own `README.txt`; field names from knowledge, unverified in
//!   2026 (research note, 28 Sep 2026).
//! - **TikTok** (Settings > Account > Download your data, JSON):
//!   `user_data_tiktok.json` (older: `user_data.json`). Videos are a
//!   `VideoList` of `{"Date": "2024-01-02 03:04:05", "Link", "Likes"}` under
//!   `Video > Videos` or, in newer files, `Post > Posts`; followers a
//!   `FansList`. Per-video views are read only if the file has them
//!   (`Views`/`VideoViews`/`PlayCount`); whether it does is **unverified**.
//!   Never retention: TikTok gives that to no one outside.
//! - **Instagram** (Accounts Centre > Your information > Download your
//!   information, JSON): `content/posts_1.json` (or under
//!   `your_instagram_activity/`), `content/reels.json`
//!   (`ig_reels_media`), `connections/followers_and_following/
//!   followers_1.json`, and for Professional accounts
//!   `past_instagram_insights/posts.json` / `reels.json`
//!   (`organic_insights_*` with `string_map_data` figures). Text in these
//!   files is UTF-8 escaped as Latin-1 (a long-standing Meta quirk) and is
//!   repaired here. Insights contents **unverified** (research: "insights-
//!   related information, when available").
//! - **LinkedIn** (Me > Posts & activity > Creator analytics > Export):
//!   an .xlsx with `DISCOVERY`, `ENGAGEMENT` (Date, Impressions,
//!   Engagements), `TOP POSTS` (Post URL, Post publish date, Engagements /
//!   Impressions, side by side), `FOLLOWERS` ("Total followers on <date>:",
//!   then Date, New followers). Source: LinkedIn Help a704175.
//! - **YouTube** (Studio > Analytics > Advanced mode > Export current view,
//!   CSV): `Table data.csv` with `Content` (the video id), `Video title`,
//!   `Video publish time`, `Duration`, `Views`, `Watch time (hours)`,
//!   `Average view duration`, `Average percentage viewed (%)`... and a
//!   `Total` row first. The figures cover **the date range that was on
//!   screen**, which the answer says.

use super::snapshots::{AccountSnap, Metrics, Platform, PostSnap, Record};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The largest export read whole into memory. X archives with media run to
/// gigabytes; past this, unzipping it and giving the folder is asked for.
pub const MAX_ZIP_BYTES: u64 = 512 * 1024 * 1024;
const MAX_PART: u64 = 256 * 1024 * 1024;
const MAX_FILES: usize = 50_000;

/// What an import found.
#[derive(Debug, Clone, PartialEq)]
pub struct Imported {
    pub platform: Platform,
    /// "your X archive"
    pub what: &'static str,
    pub records: Vec<Record>,
    /// What the file doesn't have, said plainly.
    pub missing: Vec<String>,
    pub posts: usize,
    pub account_days: usize,
    pub handle: String,
}

/// An export: a folder you unzipped, the zip itself, or one file from it.
enum Bundle {
    Dir(PathBuf, Vec<String>),
    Zip(Vec<u8>, Vec<String>),
}

impl Bundle {
    fn open(path: &Path) -> Result<Bundle, String> {
        let meta = std::fs::metadata(path).map_err(|e| format!("I can't open {}: {e}", path.display()))?;
        if meta.is_dir() {
            let mut names = Vec::new();
            walk(path, path, 0, &mut names);
            return Ok(Bundle::Dir(path.to_path_buf(), names));
        }
        let lower = path.to_string_lossy().to_lowercase();
        if lower.ends_with(".zip") || lower.ends_with(".xlsx") {
            if meta.len() > MAX_ZIP_BYTES {
                return Err(format!(
                    "that zip is {} MB -- too big to read in one go. Unzip it and give me the folder instead.",
                    meta.len() / (1024 * 1024)
                ));
            }
            let bytes = std::fs::read(path).map_err(|e| format!("I can't read {}: {e}", path.display()))?;
            let names = RefCell::new(Vec::new());
            crate::zipread::file_inside(
                &bytes,
                |n| {
                    names.borrow_mut().push(n.to_string());
                    false
                },
                MAX_PART,
            )?;
            return Ok(Bundle::Zip(bytes, names.into_inner()));
        }
        // One file on its own: a bundle of one, named as itself.
        let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        Ok(Bundle::Dir(dir, vec![name]))
    }

    fn names(&self) -> &[String] {
        match self {
            Bundle::Dir(_, n) | Bundle::Zip(_, n) => n,
        }
    }

    fn find(&self, wanted: impl Fn(&str) -> bool) -> Vec<String> {
        self.names().iter().filter(|n| wanted(&n.replace('\\', "/").to_lowercase())).cloned().collect()
    }

    fn read(&self, name: &str) -> Result<Vec<u8>, String> {
        match self {
            Bundle::Dir(root, _) => {
                let p = root.join(name);
                let len = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                if len > MAX_PART {
                    return Err(format!("{name} is bigger than I'll read ({} MB)", len / (1024 * 1024)));
                }
                std::fs::read(&p).map_err(|e| format!("I can't read {name}: {e}"))
            }
            Bundle::Zip(bytes, _) => crate::zipread::file_inside(bytes, |n| n == name, MAX_PART)?
                .map(|(_, b)| b)
                .ok_or_else(|| format!("{name} went missing from the zip")),
        }
    }

    fn text(&self, name: &str) -> Result<String, String> {
        self.read(name).map(|b| String::from_utf8_lossy(&b).into_owned())
    }
}

fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<String>) {
    if depth > 8 || out.len() >= MAX_FILES {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        // Never follow a link out of the export.
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            walk(root, &p, depth + 1, out);
        } else if let Ok(rel) = p.strip_prefix(root) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// Read an export, whichever platform it's from.
///
/// `now` stands in for "as of" only when the export doesn't say when it was
/// made and the file's own date can't be read.
pub fn import_path(path: &Path, now: u64) -> Result<Imported, String> {
    let bundle = Bundle::open(path)?;
    let as_of = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .filter(|s| *s > 0 && *s <= now)
        .unwrap_or(now);
    let names: Vec<String> = bundle.names().iter().map(|n| n.replace('\\', "/").to_lowercase()).collect();
    let has = |f: &dyn Fn(&str) -> bool| names.iter().any(|n| f(n));
    if has(&|n| n.ends_with("tweets.js") || n.ends_with("tweet.js") || (n.contains("tweets-part") && n.ends_with(".js"))) {
        return x_archive(&bundle, as_of, now);
    }
    if has(&|n| n.ends_with("table data.csv")) || (names.len() == 1 && names[0].ends_with(".csv")) {
        return youtube_studio(&bundle, as_of, now);
    }
    if has(&|n| n.ends_with(".xlsx")) || path.to_string_lossy().to_lowercase().ends_with(".xlsx") {
        return linkedin(path, &bundle, now);
    }
    if has(&|n| n.ends_with("user_data_tiktok.json") || n.ends_with("user_data.json")) {
        return tiktok(&bundle, as_of, now);
    }
    if has(&|n| {
        (n.contains("posts_") && n.ends_with(".json")) || n.ends_with("reels.json") || n.contains("past_instagram_insights") || n.contains("followers_and_following")
    }) {
        return instagram(&bundle, as_of, now);
    }
    // A single JSON with no telling name: look inside for TikTok's shape.
    if let Some(n) = bundle.find(|n| n.ends_with(".json")).first() {
        if bundle.text(n).map(|t| t.contains("VideoList")).unwrap_or(false) {
            return tiktok(&bundle, as_of, now);
        }
    }
    Err("I don't recognise that as an export from X, TikTok, Instagram, LinkedIn or YouTube Studio. \
         Give me the zip or folder the platform sent, or the file itself (tweets.js, user_data_tiktok.json, \
         the LinkedIn .xlsx, or YouTube Studio's Table data.csv)."
        .into())
}

// ---------------------------------------------------------------- shared

/// A count as exports write it: 12, "12", "1,234". Anything else is not a
/// number and is `None`, never 0.
fn num(v: &Value) -> Option<u64> {
    match v {
        Value::Number(n) => n.as_u64().or_else(|| n.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64)),
        Value::String(s) => count_text(s),
        _ => None,
    }
}

fn count_text(s: &str) -> Option<u64> {
    let t: String = s.trim().chars().filter(|c| *c != ',').collect();
    if t.is_empty() {
        return None;
    }
    t.parse::<u64>().ok().or_else(|| t.parse::<f64>().ok().filter(|f| *f >= 0.0).map(|f| f.round() as u64))
}

fn float_text(s: &str) -> Option<f64> {
    s.trim().trim_end_matches('%').replace(',', "").parse::<f64>().ok().filter(|f| f.is_finite())
}

/// Meta writes UTF-8 as if each byte were a Latin-1 character ("donâ\u{80}\u{99}t").
/// If every character fits in a byte and the bytes are valid UTF-8, that's
/// what happened, and the real text comes back.
pub fn meta_text(s: &str) -> String {
    if s.is_ascii() || s.chars().any(|c| c as u32 > 0xFF) {
        return s.to_string();
    }
    let bytes: Vec<u8> = s.chars().map(|c| c as u32 as u8).collect();
    String::from_utf8(bytes).unwrap_or_else(|_| s.to_string())
}

fn clip(s: &str) -> String {
    s.chars().take(280).collect()
}

const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

fn month_of(w: &str) -> Option<u32> {
    let w = w.to_lowercase();
    MONTHS.iter().position(|m| w.starts_with(m)).map(|i| i as u32 + 1)
}

/// "2024-01-02 03:04:05", "2024-01-02T03:04:05Z", "2024-01-02" (UTC).
fn iso_time(s: &str) -> Option<u64> {
    let s = s.trim();
    let y: i64 = s.get(0..4)?.parse().ok()?;
    let m: u32 = s.get(5..7)?.parse().ok()?;
    let d: u32 = s.get(8..10)?.parse().ok()?;
    if s.get(4..5)? != "-" || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let (hh, mm, ss) = if s.len() >= 19 {
        (s.get(11..13)?.parse::<i64>().ok()?, s.get(14..16)?.parse::<i64>().ok()?, s.get(17..19)?.parse::<i64>().ok()?)
    } else {
        (0, 0, 0)
    };
    let t = crate::civil::days_from_civil(y, m, d) * 86_400 + hh * 3600 + mm * 60 + ss;
    (t >= 0).then_some(t as u64)
}

/// "Tue Sep 24 14:03:11 +0000 2024" (X).
fn x_time(s: &str) -> Option<u64> {
    let p: Vec<&str> = s.split_whitespace().collect();
    if p.len() != 6 {
        return None;
    }
    let m = month_of(p[1])?;
    let d: u32 = p[2].parse().ok()?;
    let y: i64 = p[5].parse().ok()?;
    let hms: Vec<i64> = p[3].split(':').filter_map(|x| x.parse().ok()).collect();
    if hms.len() != 3 {
        return None;
    }
    let off = p[4];
    let sign = if off.starts_with('-') { -1 } else { 1 };
    let oh: i64 = off.get(1..3)?.parse().ok()?;
    let om: i64 = off.get(3..5)?.parse().ok()?;
    let t = crate::civil::days_from_civil(y, m, d) * 86_400 + hms[0] * 3600 + hms[1] * 60 + hms[2] - sign * (oh * 3600 + om * 60);
    (t >= 0).then_some(t as u64)
}

/// "Sep 3, 2024", "Jan 01, 2024 10:00 am", "9/24/2024" (US order, as
/// LinkedIn and Studio write them in English).
fn words_date(s: &str) -> Option<u64> {
    let s = s.trim();
    if let Some(t) = iso_time(s) {
        return Some(t);
    }
    if s.contains('/') {
        let p: Vec<&str> = s.split(|c: char| c == '/' || c.is_whitespace()).collect();
        if p.len() >= 3 {
            let m: u32 = p[0].parse().ok()?;
            let d: u32 = p[1].parse().ok()?;
            let mut y: i64 = p[2].parse().ok()?;
            if y < 100 {
                y += 2000;
            }
            if (1..=12).contains(&m) && (1..=31).contains(&d) {
                return Some((crate::civil::days_from_civil(y, m, d) * 86_400) as u64);
            }
        }
        return None;
    }
    let cleaned = s.replace(',', " ");
    let p: Vec<&str> = cleaned.split_whitespace().collect();
    if p.len() < 3 {
        return None;
    }
    let m = month_of(p[0])?;
    let d: u32 = p[1].parse().ok()?;
    let y: i64 = p[2].parse().ok()?;
    let mut secs = 0i64;
    if p.len() >= 4 {
        let hm: Vec<i64> = p[3].split(':').filter_map(|x| x.parse().ok()).collect();
        if hm.len() >= 2 {
            let mut h = hm[0] % 12;
            if p.get(4).is_some_and(|x| x.eq_ignore_ascii_case("pm")) {
                h += 12;
            }
            if p.get(4).is_none() {
                h = hm[0];
            }
            secs = h * 3600 + hm[1] * 60;
        }
    }
    Some((crate::civil::days_from_civil(y, m, d) * 86_400 + secs) as u64)
}

/// "0:01:23", "1:23", "83" -> seconds.
fn clock_secs(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.trim().split(':').collect();
    let mut total = 0.0;
    for p in &parts {
        total = total * 60.0 + p.trim().parse::<f64>().ok()?;
    }
    Some(total)
}

/// The JSON array in a `window.YTD.x.part0 = [...]` file.
fn ytd_array(js: &str) -> Result<Vec<Value>, String> {
    let start = js.find('[').ok_or("no list in it")?;
    serde_json::from_str::<Vec<Value>>(js[start..].trim().trim_end_matches(';')).map_err(|e| format!("it doesn't read: {e}"))
}

fn post(platform: Platform, id: String, posted: Option<u64>, text: &str, url: String, seconds: Option<f64>, source: &str, as_of: u64, now: u64, m: Metrics) -> Record {
    Record::Post(PostSnap { platform, id, day: super::social_day(as_of), taken: now, posted, text: clip(text), url, seconds, source: source.into(), m })
}

// ---------------------------------------------------------------- X

/// The folder an X archive keeps its files in -- X's layout, inside the
/// archive you downloaded, not anything of Atlas's.
///
/// Named on its own (29 Sep 2026) because `tests/one_install_root.rs` reads a
/// `"data/..."` literal as one of Atlas's own install paths, which only
/// `roots` and `store` may spell. These are not: they are names inside
/// somebody else's zip, matched against the archive's own entry names.
const X_ARCHIVE_FOLDER: &str = "data";

/// Is entry `n` of an X archive the file `name`, either in the archive's own
/// folder or loose (a folder of just the `.js` files)?
fn in_x_archive(n: &str, name: &str) -> bool {
    n == name || n.ends_with(&format!("{X_ARCHIVE_FOLDER}/{name}"))
}

fn x_archive(b: &Bundle, as_of: u64, now: u64) -> Result<Imported, String> {
    let mut as_of = as_of;
    if let Some(m) = b.find(|n| in_x_archive(n, "manifest.js")).first() {
        let text = b.text(m).unwrap_or_default();
        if let Some(i) = text.find("\"generationDate\"") {
            let rest = &text[i + 16..];
            if let Some(q) = rest.find('"') {
                let v = &rest[q + 1..];
                if let Some(e) = v.find('"') {
                    if let Some(t) = iso_time(&v[..e]) {
                        as_of = t;
                    }
                }
            }
        }
    }
    let mut handle = String::new();
    if let Some(a) = b.find(|n| in_x_archive(n, "account.js")).first() {
        if let Ok(arr) = ytd_array(&b.text(a)?) {
            handle = arr.first().and_then(|v| v.pointer("/account/username")).and_then(|v| v.as_str()).unwrap_or("").to_string();
        }
    }
    let mut records = Vec::new();
    let mut retweets = 0usize;
    let mut seen = std::collections::HashSet::new();
    let mut files = b.find(|n| n.ends_with("tweets.js") || n.ends_with("tweet.js") || (n.contains("tweets-part") && n.ends_with(".js")));
    files.sort();
    for f in files {
        for v in ytd_array(&b.text(&f)?).map_err(|e| format!("{f}: {e}"))? {
            let t = v.get("tweet").unwrap_or(&v);
            let id = t.get("id_str").or_else(|| t.get("id")).and_then(|x| x.as_str()).unwrap_or("").to_string();
            let text = t.get("full_text").or_else(|| t.get("text")).and_then(|x| x.as_str()).unwrap_or("");
            if id.is_empty() || !seen.insert(id.clone()) {
                continue;
            }
            // A repost of someone else's post carries their numbers, not yours.
            if text.starts_with("RT @") {
                retweets += 1;
                continue;
            }
            let m = Metrics { likes: t.get("favorite_count").and_then(num), reposts: t.get("retweet_count").and_then(num), ..Default::default() };
            let posted = t.get("created_at").and_then(|x| x.as_str()).and_then(x_time);
            let url = if handle.is_empty() { format!("https://x.com/i/web/status/{id}") } else { format!("https://x.com/{handle}/status/{id}") };
            records.push(post(Platform::X, id, posted, text, url, None, "your X archive", as_of, now, m));
        }
    }
    let posts = records.len();
    let mut account_days = 0;
    if let Some(f) = b.find(|n| in_x_archive(n, "follower.js")).first() {
        let followers = ytd_array(&b.text(f)?)?.len() as u64;
        records.push(Record::Account(AccountSnap {
            platform: Platform::X,
            handle: handle.clone(),
            day: super::social_day(as_of),
            taken: now,
            followers: Some(followers),
            following: None,
            posts: Some(posts as u64),
            source: "your X archive".into(),
            m: Metrics::default(),
        }));
        account_days = 1;
    }
    let mut missing = vec!["impressions and views (X gives those only with Premium)".to_string()];
    if retweets > 0 {
        missing.push(format!("{retweets} reposts of other people's posts, left out -- their numbers aren't yours"));
    }
    if account_days == 0 {
        missing.push("your follower count (the archive had no follower.js)".into());
    }
    Ok(Imported { platform: Platform::X, what: "your X archive", records, missing, posts, account_days, handle })
}

// ---------------------------------------------------------------- TikTok

/// Every array under a key named `key`, anywhere in the document.
fn arrays_named<'a>(v: &'a Value, key: &str, out: &mut Vec<&'a Vec<Value>>) {
    match v {
        Value::Object(map) => {
            for (k, child) in map {
                if k == key {
                    if let Value::Array(a) = child {
                        out.push(a);
                        continue;
                    }
                }
                arrays_named(child, key, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|c| arrays_named(c, key, out)),
        _ => {}
    }
}

fn first_str<'a>(v: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|k| v.get(*k).and_then(|x| x.as_str()).filter(|s| !s.trim().is_empty()))
}

fn first_num(v: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|k| v.get(*k).and_then(num))
}

fn tiktok(b: &Bundle, as_of: u64, now: u64) -> Result<Imported, String> {
    let name = b
        .find(|n| n.ends_with("user_data_tiktok.json") || n.ends_with("user_data.json"))
        .into_iter()
        .next()
        .or_else(|| b.find(|n| n.ends_with(".json")).into_iter().next())
        .ok_or("no TikTok data file in it")?;
    let doc: Value = serde_json::from_str(&b.text(&name)?).map_err(|e| format!("{name} doesn't read: {e}"))?;
    let handle = ["/Profile/Profile Information/ProfileMap/userName", "/Profile/Profile Info/ProfileMap/userName", "/Profile/ProfileMap/userName"]
        .iter()
        .find_map(|p| doc.pointer(p).and_then(|v| v.as_str()))
        .unwrap_or("")
        .to_string();
    let mut lists = Vec::new();
    arrays_named(&doc, "VideoList", &mut lists);
    let mut records = Vec::new();
    let mut with_views = 0;
    let mut seen = std::collections::HashSet::new();
    for list in lists {
        for v in list {
            let link = first_str(v, &["Link", "VideoLink", "link"]).unwrap_or("").to_string();
            let posted = first_str(v, &["Date", "date"]).and_then(iso_time);
            // The video's number is the last run of digits in its link.
            let id = link
                .trim_end_matches('/')
                .rsplit('/')
                .find(|seg| seg.len() > 5 && seg.chars().all(|c| c.is_ascii_digit()))
                .map(str::to_string)
                .unwrap_or_else(|| link.clone());
            if id.is_empty() || !seen.insert(id.clone()) {
                continue;
            }
            let m = Metrics {
                likes: first_num(v, &["Likes", "LikeCount", "likes"]),
                views: first_num(v, &["Views", "VideoViews", "PlayCount", "views"]),
                comments: first_num(v, &["Comments", "CommentCount"]),
                shares: first_num(v, &["Shares", "ShareCount"]),
                ..Default::default()
            };
            if m.views.is_some() {
                with_views += 1;
            }
            let text = first_str(v, &["Title", "Description", "AboutVideo", "Caption"]).unwrap_or("");
            records.push(post(Platform::Tiktok, id, posted, text, link, None, "your TikTok data export", as_of, now, m));
        }
    }
    let posts = records.len();
    let mut fans = Vec::new();
    arrays_named(&doc, "FansList", &mut fans);
    let mut following = Vec::new();
    arrays_named(&doc, "Following", &mut following);
    let mut account_days = 0;
    if let Some(f) = fans.first() {
        records.push(Record::Account(AccountSnap {
            platform: Platform::Tiktok,
            handle: handle.clone(),
            day: super::social_day(as_of),
            taken: now,
            followers: Some(f.len() as u64),
            following: following.first().map(|l| l.len() as u64),
            posts: Some(posts as u64),
            source: "your TikTok data export".into(),
            m: Metrics::default(),
        }));
        account_days = 1;
    }
    let mut missing = vec!["watch time and retention (TikTok gives those to no one outside TikTok)".to_string()];
    if posts > 0 && with_views == 0 {
        missing.push("views per video (this export doesn't include them)".into());
    }
    if posts == 0 {
        missing.push("your videos (the export has no video list -- ask TikTok for \"Posts\" when you request it)".into());
    }
    if account_days == 0 {
        missing.push("your follower count (no follower list in the export)".into());
    }
    Ok(Imported { platform: Platform::Tiktok, what: "your TikTok data export", records, missing, posts, account_days, handle })
}

// ---------------------------------------------------------------- Instagram

fn ig_media_list(v: &Value) -> Vec<&Value> {
    match v {
        Value::Array(a) => a.iter().collect(),
        Value::Object(o) => o.values().filter_map(|x| x.as_array()).flatten().collect(),
        _ => Vec::new(),
    }
}

/// "123", "1,234" from `string_map_data`'s `{"value": ...}`, found by any of
/// the names Meta has used for it.
fn ig_figure(map: &Value, names: &[&str]) -> Option<u64> {
    let o = map.as_object()?;
    for (k, v) in o {
        if names.iter().any(|n| k.eq_ignore_ascii_case(n)) {
            if let Some(n) = v.get("value").and_then(num) {
                return Some(n);
            }
        }
    }
    None
}

fn instagram(b: &Bundle, as_of: u64, now: u64) -> Result<Imported, String> {
    // Keyed by when it was posted: the export has no stable id, and a post
    // and its insights meet on the time.
    let mut by_time: BTreeMap<u64, PostSnap> = BTreeMap::new();
    let source = "your Instagram data download";
    let mut add = |t: u64, text: &str, uri: &str, reel: bool, m: Metrics| {
        let e = by_time.entry(t).or_insert_with(|| PostSnap {
            platform: Platform::Instagram,
            id: format!("ig-{t}"),
            day: super::social_day(as_of),
            taken: now,
            posted: Some(t),
            text: clip(&meta_text(text)),
            url: String::new(),
            seconds: None,
            source: source.into(),
            m: Metrics::default(),
        });
        if e.text.is_empty() {
            e.text = clip(&meta_text(text));
        }
        if e.url.is_empty() && reel && !uri.is_empty() {
            e.url = uri.to_string();
        }
        e.m.fill_from(&m);
    };
    for f in b.find(|n| (n.contains("/posts_") || n.starts_with("posts_")) && n.ends_with(".json") && !n.contains("insights")) {
        let v: Value = serde_json::from_str(&b.text(&f)?).map_err(|e| format!("{f} doesn't read: {e}"))?;
        for item in ig_media_list(&v) {
            let media = item.get("media").and_then(|m| m.as_array());
            let first = media.and_then(|m| m.first());
            let t = item.get("creation_timestamp").and_then(num).or_else(|| first.and_then(|m| m.get("creation_timestamp")).and_then(num));
            let text = item.get("title").and_then(|x| x.as_str()).filter(|s| !s.is_empty()).or_else(|| first.and_then(|m| m.get("title")).and_then(|x| x.as_str())).unwrap_or("");
            if let Some(t) = t {
                add(t, text, "", false, Metrics::default());
            }
        }
    }
    for f in b.find(|n| n.ends_with("reels.json") && !n.contains("insights")) {
        let v: Value = serde_json::from_str(&b.text(&f)?).map_err(|e| format!("{f} doesn't read: {e}"))?;
        for item in ig_media_list(&v) {
            for m in item.get("media").and_then(|m| m.as_array()).into_iter().flatten() {
                if let Some(t) = m.get("creation_timestamp").and_then(num) {
                    add(t, m.get("title").and_then(|x| x.as_str()).unwrap_or(""), "", true, Metrics::default());
                }
            }
        }
    }
    let mut insights = 0;
    for f in b.find(|n| n.contains("past_instagram_insights") && n.ends_with(".json")) {
        let v: Value = serde_json::from_str(&b.text(&f)?).map_err(|e| format!("{f} doesn't read: {e}"))?;
        for item in ig_media_list(&v) {
            let Some(figs) = item.get("string_map_data") else { continue };
            let thumb = item.pointer("/media_map_data/Media Thumbnail");
            let t = thumb
                .and_then(|x| x.get("creation_timestamp"))
                .and_then(num)
                .or_else(|| figs.get("Creation Timestamp").and_then(|x| x.get("value")).and_then(|x| x.as_str()).and_then(words_date));
            let Some(t) = t else { continue };
            let m = Metrics {
                views: ig_figure(figs, &["Views", "Plays", "Video Views"]),
                impressions: ig_figure(figs, &["Impressions"]),
                reach: ig_figure(figs, &["Accounts reached", "Reach"]),
                likes: ig_figure(figs, &["Likes"]),
                comments: ig_figure(figs, &["Comments"]),
                saves: ig_figure(figs, &["Saves", "Saved"]),
                shares: ig_figure(figs, &["Shares"]),
                ..Default::default()
            };
            insights += 1;
            add(t, thumb.and_then(|x| x.get("title")).and_then(|x| x.as_str()).unwrap_or(""), "", false, m);
        }
    }
    let mut records: Vec<Record> = by_time.into_values().map(Record::Post).collect();
    let posts = records.len();
    let mut followers = None;
    for f in b.find(|n| n.contains("followers_and_following/followers") && n.ends_with(".json")) {
        let v: Value = serde_json::from_str(&b.text(&f)?).map_err(|e| format!("{f} doesn't read: {e}"))?;
        *followers.get_or_insert(0u64) += ig_media_list(&v).len() as u64;
    }
    let mut account_days = 0;
    if followers.is_some() {
        records.push(Record::Account(AccountSnap {
            platform: Platform::Instagram,
            handle: String::new(),
            day: super::social_day(as_of),
            taken: now,
            followers,
            following: None,
            posts: Some(posts as u64),
            source: source.into(),
            m: Metrics::default(),
        }));
        account_days = 1;
    }
    let mut missing = Vec::new();
    if insights == 0 {
        missing.push("per-post views, reach and likes (only a Professional account's download has insights; the Instagram API gives them daily)".into());
    }
    if account_days == 0 {
        missing.push("your follower count (no followers file in the download)".into());
    }
    Ok(Imported { platform: Platform::Instagram, what: "your Instagram data download", records, missing, posts, account_days, handle: String::new() })
}

// ---------------------------------------------------------------- LinkedIn

fn cell_day(s: &str) -> Option<i64> {
    let t = s.trim();
    if let Ok(serial) = t.parse::<f64>() {
        if (20_000.0..80_000.0).contains(&serial) {
            return Some(super::xlsx::excel_day(serial));
        }
    }
    words_date(t).map(super::social_day)
}

fn header_at(rows: &[Vec<String>], wants: &[&str]) -> Option<usize> {
    rows.iter().position(|r| wants.iter().all(|w| r.iter().any(|c| c.trim().eq_ignore_ascii_case(w))))
}

fn col(row: &[String], name: &str, from: usize) -> Option<usize> {
    row.iter().enumerate().skip(from).find(|(_, c)| c.trim().eq_ignore_ascii_case(name)).map(|(i, _)| i)
}

fn linkedin(path: &Path, b: &Bundle, now: u64) -> Result<Imported, String> {
    let bytes = if path.to_string_lossy().to_lowercase().ends_with(".xlsx") {
        std::fs::read(path).map_err(|e| format!("I can't read it: {e}"))?
    } else {
        let f = b.find(|n| n.ends_with(".xlsx")).into_iter().next().ok_or("no spreadsheet in it")?;
        b.read(&f)?
    };
    let sheets = super::xlsx::sheets(&bytes)?;
    // "Content_2024-09-01_2024-09-28_JordanLee.xlsx": the name is its last part.
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let handle = stem.rsplit('_').next().filter(|h| h.chars().any(|c| c.is_alphabetic()) && stem.contains('_')).unwrap_or("").to_string();
    let source = "your LinkedIn analytics export";
    let mut days: BTreeMap<i64, AccountSnap> = BTreeMap::new();
    let day_entry = |day: i64| -> AccountSnap {
        AccountSnap { platform: Platform::Linkedin, handle: handle.clone(), day, taken: now, followers: None, following: None, posts: None, source: source.into(), m: Metrics::default() }
    };
    let mut posts: BTreeMap<String, PostSnap> = BTreeMap::new();
    let mut last_day = super::social_day(now);
    for sh in &sheets {
        let rows = &sh.rows;
        // Daily impressions and engagements.
        if let Some(h) = header_at(rows, &["Date", "Impressions", "Engagements"]) {
            let (dc, ic, ec) = (col(&rows[h], "Date", 0).unwrap_or(0), col(&rows[h], "Impressions", 0).unwrap_or(1), col(&rows[h], "Engagements", 0).unwrap_or(2));
            for r in &rows[h + 1..] {
                let Some(day) = r.get(dc).and_then(|c| cell_day(c)) else { continue };
                let e = days.entry(day).or_insert_with(|| day_entry(day));
                e.m.impressions = r.get(ic).and_then(|c| count_text(c));
                e.m.engagements = r.get(ec).and_then(|c| count_text(c));
            }
        }
        // Followers: the total on the export's last day, then new ones a day.
        for r in rows {
            if let Some(label) = r.first().filter(|c| c.to_lowercase().starts_with("total followers on")) {
                let date = label.to_lowercase().trim_start_matches("total followers on").trim().trim_end_matches(':').to_string();
                if let (Some(day), Some(n)) = (words_date(&date).map(super::social_day), r.get(1).and_then(|c| count_text(c))) {
                    last_day = day;
                    days.entry(day).or_insert_with(|| day_entry(day)).followers = Some(n);
                }
            }
        }
        if let Some(h) = header_at(rows, &["Date", "New followers"]) {
            let (dc, nc) = (col(&rows[h], "Date", 0).unwrap_or(0), col(&rows[h], "New followers", 0).unwrap_or(1));
            for r in &rows[h + 1..] {
                let Some(day) = r.get(dc).and_then(|c| cell_day(c)) else { continue };
                if let Some(n) = r.get(nc).and_then(|c| count_text(c)) {
                    days.entry(day).or_insert_with(|| day_entry(day)).m.followers_gained = Some(n as i64);
                }
            }
        }
        // Top posts: two tables side by side, joined on the post's address.
        if let Some(h) = header_at(rows, &["Post URL", "Post publish date"]) {
            let head = &rows[h];
            let mut from = 0;
            while let Some(uc) = col(head, "Post URL", from) {
                let dc = col(head, "Post publish date", uc);
                let ec = col(head, "Engagements", uc).filter(|c| *c <= uc + 3);
                let ic = col(head, "Impressions", uc).filter(|c| *c <= uc + 3);
                for r in &rows[h + 1..] {
                    let Some(url) = r.get(uc).map(|u| u.trim()).filter(|u| u.starts_with("http")) else { continue };
                    let id = url.rsplit(':').next().unwrap_or(url).trim_end_matches('/').to_string();
                    let p = posts.entry(id.clone()).or_insert_with(|| PostSnap {
                        platform: Platform::Linkedin,
                        id,
                        day: last_day,
                        taken: now,
                        posted: None,
                        text: String::new(),
                        url: url.to_string(),
                        seconds: None,
                        source: source.into(),
                        m: Metrics::default(),
                    });
                    if p.posted.is_none() {
                        p.posted = dc.and_then(|c| r.get(c)).and_then(|c| cell_day(c)).map(|d| (d * 86_400) as u64);
                    }
                    if let Some(n) = ec.and_then(|c| r.get(c)).and_then(|c| count_text(c)) {
                        p.m.engagements = Some(n);
                    }
                    if let Some(n) = ic.and_then(|c| r.get(c)).and_then(|c| count_text(c)) {
                        p.m.impressions = Some(n);
                    }
                }
                from = uc + 1;
            }
        }
    }
    let mut records: Vec<Record> = Vec::new();
    let n_posts = posts.len();
    for mut p in posts.into_values() {
        p.day = last_day;
        records.push(Record::Post(p));
    }
    let account_days = days.len();
    records.extend(days.into_values().map(Record::Account));
    if records.is_empty() {
        return Err("that spreadsheet isn't a LinkedIn analytics export I recognise (no Date/Impressions, followers or top posts sheet)".into());
    }
    let missing = vec![
        "your posts' text (the export lists top posts by address only)".to_string(),
        "anything beyond your top 50 posts in the chosen range".to_string(),
    ];
    Ok(Imported { platform: Platform::Linkedin, what: "your LinkedIn analytics export", records, missing, posts: n_posts, account_days, handle })
}

// ---------------------------------------------------------------- YouTube Studio

/// A CSV line into fields: commas, quotes, doubled quotes inside quotes.
pub fn csv_rows(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => row.push(std::mem::take(&mut field)),
            '\r' if !quoted => {}
            '\n' if !quoted => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

fn youtube_studio(b: &Bundle, as_of: u64, now: u64) -> Result<Imported, String> {
    let name = b
        .find(|n| n.ends_with("table data.csv"))
        .into_iter()
        .next()
        .or_else(|| b.find(|n| n.ends_with(".csv")).into_iter().next())
        .ok_or("no Table data.csv in it")?;
    let rows = csv_rows(&b.text(&name)?);
    let head = rows.first().ok_or("that CSV is empty")?;
    let find = |names: &[&str]| head.iter().position(|h| names.iter().any(|n| h.trim().eq_ignore_ascii_case(n)));
    let id_c = find(&["Content", "Video"]).ok_or("that CSV has no Content column, so it isn't a Studio table export")?;
    let (title_c, when_c, dur_c) = (find(&["Video title"]), find(&["Video publish time", "Video publish date"]), find(&["Duration"]));
    let (views_c, watch_c, avd_c, apv_c) =
        (find(&["Views"]), find(&["Watch time (hours)"]), find(&["Average view duration"]), find(&["Average percentage viewed (%)"]));
    let (subs_c, imp_c, likes_c, comm_c, shares_c) = (find(&["Subscribers"]), find(&["Impressions"]), find(&["Likes"]), find(&["Comments added", "Comments"]), find(&["Shares"]));
    let get = |r: &Vec<String>, c: Option<usize>| c.and_then(|c| r.get(c)).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let mut records = Vec::new();
    for r in &rows[1..] {
        let Some(id) = get(r, Some(id_c)) else { continue };
        if id.eq_ignore_ascii_case("total") {
            continue;
        }
        let m = Metrics {
            views: get(r, views_c).and_then(|s| count_text(&s)),
            watch_minutes: get(r, watch_c).and_then(|s| float_text(&s)).map(|h| h * 60.0),
            avg_view_secs: get(r, avd_c).and_then(|s| clock_secs(&s)),
            avg_view_pct: get(r, apv_c).and_then(|s| float_text(&s)),
            followers_gained: get(r, subs_c).and_then(|s| s.replace(',', "").parse::<i64>().ok()),
            impressions: get(r, imp_c).and_then(|s| count_text(&s)),
            likes: get(r, likes_c).and_then(|s| count_text(&s)),
            comments: get(r, comm_c).and_then(|s| count_text(&s)),
            shares: get(r, shares_c).and_then(|s| count_text(&s)),
            ..Default::default()
        };
        let posted = get(r, when_c).and_then(|s| words_date(&s));
        let seconds = get(r, dur_c).and_then(|s| float_text(&s));
        let url = format!("https://www.youtube.com/watch?v={id}");
        records.push(post(Platform::Youtube, id, posted, &get(r, title_c).unwrap_or_default(), url, seconds, "your YouTube Studio export (the date range on screen)", as_of, now, m));
    }
    let posts = records.len();
    let mut missing = vec!["your subscriber total (Studio's table has per-video gains, not the total)".to_string()];
    if apv_c.is_none() && avd_c.is_none() {
        missing.push("retention (add \"Average view duration\" or \"Average percentage viewed\" as a column before exporting)".into());
    }
    Ok(Imported { platform: Platform::Youtube, what: "your YouTube Studio export", records, missing, posts, account_days: 0, handle: String::new() })
}
