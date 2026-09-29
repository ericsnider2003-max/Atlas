//! What the numbers say, and only what they say.
//!
//! Every figure in an answer here is read from a snapshot or computed from
//! snapshots in a way the answer states (a median, a ratio, a difference).
//! Where a platform's data is missing, the answer names the platform and
//! what's missing instead of leaving it out quietly -- "combined" means the
//! platforms that have the figure, and says which ones don't.
//!
//! "Why it worked" is tied to `content`'s reasons -- the kind of opening
//! (`content::hook_of` on the title or caption), the length, and retention
//! where a platform gives it -- by comparing the posts that did best against
//! the rest. Under `min_posts` in the window that is noise, and the answer
//! says so rather than finding a pattern in it. The one retention figure
//! `content` cares most about, still watching at three seconds, is not given
//! free by any platform, and that's said too.

use super::snapshots::{Book, Platform, PostSnap};
use super::watchlist::Watch;
use crate::tz::Zone;

fn median(v: &mut [f64]) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let m = v.len() / 2;
    Some(if v.len() % 2 == 1 { v[m] } else { (v[m - 1] + v[m]) / 2.0 })
}

/// The one number a post is judged on, per platform, and its name: views
/// where the platform gives them, impressions next, interactions last.
pub fn headline_figure(p: &PostSnap) -> Option<(&'static str, u64)> {
    if let Some(v) = p.m.views {
        return Some(("views", v));
    }
    if let Some(v) = p.m.impressions {
        return Some(("impressions", v));
    }
    p.m.interactions().map(|v| ("interactions", v))
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("").trim()
}

fn plural(n: usize, one: &str) -> String {
    format!("{n} {one}{}", if n == 1 { "" } else { "s" })
}

fn name_list(ps: &[Platform]) -> String {
    let names: Vec<&str> = ps.iter().map(|p| p.name()).collect();
    match names.len() {
        0 => String::new(),
        1 => names[0].to_string(),
        n => format!("{} and {}", names[..n - 1].join(", "), names[n - 1]),
    }
}

/// The platforms with nothing kept, named, for the end of an answer.
fn missing_platforms(book: &Book, among: &[Platform]) -> Option<String> {
    let have = book.platforms();
    let none: Vec<Platform> = among.iter().copied().filter(|p| !have.contains(p)).collect();
    (!none.is_empty()).then(|| format!("Nothing from {} yet.", name_list(&none)))
}

fn describe(p: &PostSnap) -> String {
    let mut parts = Vec::new();
    for (label, v) in [("views", p.m.views), ("impressions", p.m.impressions), ("likes", p.m.likes), ("comments", p.m.comments), ("shares", p.m.shares), ("saves", p.m.saves), ("reposts", p.m.reposts)] {
        if let Some(v) = v {
            parts.push(format!("{} {label}", super::count_said(v)));
        }
    }
    if let Some(pct) = p.m.avg_view_pct {
        parts.push(format!("{pct:.0}% watched on average"));
    } else if let Some(s) = p.m.avg_view_secs {
        parts.push(format!("{s:.0}s watched on average"));
    }
    if parts.is_empty() {
        "no figures in the data".into()
    } else {
        parts.join(", ")
    }
}

fn title_of(p: &PostSnap) -> String {
    let t = first_line(&p.text);
    if t.is_empty() {
        format!("your {} post of {}", p.platform.name(), p.posted.map(|t| super::day_label(super::social_day(t))).unwrap_or_else(|| "an unknown day".into()))
    } else {
        format!("\"{}\"", t.chars().take(70).collect::<String>())
    }
}

// ---------------------------------------------------------------- your last video

/// "How did my last video do?"
pub fn last_video(book: &Book, only: Option<Platform>, now: u64) -> String {
    let posts = book.latest_posts();
    let pick = |p: &&&PostSnap| only.map_or(p.platform.is_video(), |o| p.platform == o) && p.posted.is_some();
    let Some(last) = posts.iter().filter(pick).max_by_key(|p| p.posted) else {
        let where_ = only.map(|p| p.name().to_string()).unwrap_or_else(|| "YouTube, TikTok or Instagram".into());
        return format!("I have no videos of yours from {where_} yet. Import the platform's export on the Social page, or turn on the daily refresh for YouTube or Instagram.");
    };
    let posted = last.posted.unwrap_or(0);
    let age_days = now.saturating_sub(posted) / 86_400;
    let mut s = format!(
        "Your latest on {}, {} (posted {}): {}, as of {} ({}).",
        last.platform.name(),
        title_of(last),
        super::day_label(super::social_day(posted)),
        describe(last),
        super::day_label(last.day),
        last.source
    );
    // Its own growth, where there's more than one day of it.
    let hist = book.history(last.platform, &last.id);
    if hist.len() >= 2 {
        if let (Some(a), Some(b)) = (headline_figure(hist[0]), headline_figure(hist[hist.len() - 1])) {
            if a.0 == b.0 && b.1 >= a.1 {
                s.push_str(&format!(" Up {} {} since {}.", super::count_said(b.1 - a.1), a.0, super::day_label(hist[0].day)));
            }
        }
    }
    // Against the ten before it on the same platform: at the same age if
    // the record has them at that age, otherwise as they stand, said so.
    let Some((what, mine)) = headline_figure(last) else {
        s.push_str(" There's nothing to compare it on.");
        return s;
    };
    let mut before: Vec<&&PostSnap> = posts.iter().filter(|p| p.platform == last.platform && p.posted.is_some_and(|t| t < posted)).collect();
    before.sort_by_key(|p| std::cmp::Reverse(p.posted));
    before.truncate(10);
    let mut same_age: Vec<f64> = Vec::new();
    for p in &before {
        let day0 = super::social_day(p.posted.unwrap_or(0));
        let at = book
            .history(p.platform, &p.id)
            .into_iter()
            .filter(|h| (h.day - day0 - age_days as i64).abs() <= 1)
            .filter_map(|h| headline_figure(h).filter(|x| x.0 == what).map(|x| x.1 as f64))
            .next();
        if let Some(v) = at {
            same_age.push(v);
        }
    }
    if same_age.len() >= 3 {
        let n = same_age.len();
        let m = median(&mut same_age).unwrap_or(0.0);
        s.push_str(&format!(" Your previous {n} had a median of {} {what} at the same age", super::count_said(m as u64)));
        if m > 0.0 {
            s.push_str(&format!(" -- this one is at {:.1}x.", mine as f64 / m));
        } else {
            s.push('.');
        }
    } else {
        let mut now_vals: Vec<f64> = before.iter().filter_map(|p| headline_figure(p).filter(|x| x.0 == what).map(|x| x.1 as f64)).collect();
        if now_vals.len() >= 3 {
            let n = now_vals.len();
            let m = median(&mut now_vals).unwrap_or(0.0);
            s.push_str(&format!(
                " Your previous {n} have a median of {} {what} as they stand now -- they've had longer, so that's not a like-for-like comparison yet.",
                super::count_said(m as u64)
            ));
        } else {
            s.push_str(" Too few earlier posts on record to compare it with.");
        }
    }
    if last.platform == Platform::Tiktok {
        s.push_str(" TikTok gives no watch time or retention, so how well it held attention isn't known.");
    }
    s
}

// ---------------------------------------------------------------- what worked

/// Each post's headline against its platform's median over the same window
/// -- so a TikTok and a LinkedIn post can be ranked together.
fn against_median<'a>(posts: &[&'a PostSnap]) -> Vec<(&'a PostSnap, &'static str, u64, f64)> {
    let mut out = Vec::new();
    for pf in Platform::ALL {
        let mine: Vec<(&PostSnap, &'static str, u64)> = posts.iter().filter(|p| p.platform == pf).filter_map(|p| headline_figure(p).map(|(w, v)| (*p, w, v))).collect();
        // Only the metric most of this platform's posts share.
        let Some(what) = ["views", "impressions", "interactions"].into_iter().max_by_key(|w| mine.iter().filter(|x| x.1 == *w).count()) else { continue };
        let same: Vec<_> = mine.into_iter().filter(|x| x.1 == what).collect();
        let mut vals: Vec<f64> = same.iter().map(|x| x.2 as f64).collect();
        let Some(m) = median(&mut vals) else { continue };
        for (p, w, v) in same {
            out.push((p, w, v, if m > 0.0 { v as f64 / m } else { 0.0 }));
        }
    }
    out
}

/// "What worked this month, and why?"
pub fn what_worked(book: &Book, only: Option<Platform>, days: u64, now: u64, min_posts: usize) -> String {
    let since = now.saturating_sub(days * 86_400);
    let latest = book.latest_posts();
    let window: Vec<&PostSnap> = latest.into_iter().filter(|p| only.is_none_or(|o| p.platform == o) && p.posted.is_some_and(|t| t >= since && t <= now)).collect();
    let where_ = only.map(|p| format!(" on {}", p.name())).unwrap_or_default();
    if window.is_empty() {
        let mut s = format!("Nothing you posted{where_} in the last {days} days is on record.");
        if let Some(m) = missing_platforms(book, &only.map(|p| vec![p]).unwrap_or_else(|| Platform::ALL.to_vec())) {
            s.push(' ');
            s.push_str(&m);
        }
        return s;
    }
    let mut ranked = against_median(&window);
    ranked.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));
    let mut s = format!("In the last {days} days{where_}, {} on record.", plural(window.len(), "post"));
    let best: Vec<String> = ranked
        .iter()
        .take(3)
        .map(|(p, w, v, r)| format!("{} on {} -- {} {w}, {r:.1}x your {} median", title_of(p), p.platform.name(), super::count_said(*v), p.platform.name()))
        .collect();
    if !best.is_empty() {
        s.push_str(&format!(" Best: {}.", best.join("; ")));
    }
    if ranked.len() < min_posts {
        s.push_str(&format!(
            " That's under {min_posts} posts with figures, so any pattern I named would be noise -- the ones above are just the numbers."
        ));
    } else {
        let third = (ranked.len() / 3).max(1);
        let (top, rest) = ranked.split_at(third);
        s.push_str(&reasons(top, rest));
    }
    let with: Vec<Platform> = {
        let mut v: Vec<Platform> = window.iter().map(|p| p.platform).collect();
        v.sort();
        v.dedup();
        v
    };
    let no_retention: Vec<Platform> = with.iter().copied().filter(|p| !window.iter().any(|w| w.platform == *p && (w.m.avg_view_pct.is_some() || w.m.avg_view_secs.is_some()))).collect();
    if !no_retention.is_empty() {
        s.push_str(&format!(" No retention figures for {}", name_list(&no_retention)));
        s.push_str(if no_retention.contains(&Platform::Tiktok) || no_retention.contains(&Platform::X) { " (TikTok and X don't give them at all)." } else { "." });
    }
    s.push_str(" Nobody gives \"still watching at three seconds\" for free, so the opening is judged by its kind, not its hold.");
    if only.is_none() {
        if let Some(m) = missing_platforms(book, &Platform::ALL) {
            s.push(' ');
            s.push_str(&m);
        }
    }
    s
}

/// The top third against the rest, on `content`'s reasons.
fn reasons(top: &[(&PostSnap, &'static str, u64, f64)], rest: &[(&PostSnap, &'static str, u64, f64)]) -> String {
    let mut out = String::new();
    // The opening.
    let hook_count = |set: &[(&PostSnap, &'static str, u64, f64)], h: crate::content::Hook| set.iter().filter(|x| crate::content::hook_of(first_line(&x.0.text)) == h).count();
    let with_text = |set: &[(&PostSnap, &'static str, u64, f64)]| set.iter().filter(|x| !x.0.text.trim().is_empty()).count();
    if with_text(top) > 0 && with_text(rest) > 0 {
        use crate::content::Hook;
        let mut best: Option<(crate::content::Hook, f64, usize, usize)> = None;
        for h in [Hook::Called, Hook::Contradiction, Hook::Unfinished, Hook::Outcome, Hook::Question, Hook::None] {
            let (a, b) = (hook_count(top, h), hook_count(rest, h));
            let lift = a as f64 / with_text(top) as f64 - b as f64 / with_text(rest) as f64;
            if a >= 2 && best.is_none_or(|x| lift > x.1) {
                best = Some((h, lift, a, b));
            }
        }
        if let Some((h, _, a, b)) = best.filter(|x| x.1 > 0.15) {
            out.push_str(&format!(
                " Why, as far as the record shows: {a} of your top {} open with one that {} (\"{}\"-style), against {b} of the other {}.",
                top.len(),
                h.plain(),
                format!("{h:?}").to_lowercase(),
                rest.len()
            ));
        }
    }
    // The length.
    let mut ts: Vec<f64> = top.iter().filter_map(|x| x.0.seconds).collect();
    let mut rs: Vec<f64> = rest.iter().filter_map(|x| x.0.seconds).collect();
    if ts.len() >= 2 && rs.len() >= 2 {
        let (a, b) = (median(&mut ts).unwrap_or(0.0), median(&mut rs).unwrap_or(0.0));
        out.push_str(&format!(" Length: the best ran a median {a:.0}s, the rest {b:.0}s."));
    }
    // Retention.
    let mut tp: Vec<f64> = top.iter().filter_map(|x| x.0.m.avg_view_pct).collect();
    let mut rp: Vec<f64> = rest.iter().filter_map(|x| x.0.m.avg_view_pct).collect();
    if tp.len() >= 2 && rp.len() >= 2 {
        let (a, b) = (median(&mut tp).unwrap_or(0.0), median(&mut rp).unwrap_or(0.0));
        out.push_str(&format!(" Retention: the best were watched {a:.0}% through on average, the rest {b:.0}% -- retention is the number that compounds."));
    }
    // Interactions per view.
    let rate = |set: &[(&PostSnap, &'static str, u64, f64)]| {
        let mut v: Vec<f64> = set.iter().filter_map(|x| Some(x.0.m.interactions()? as f64 / x.0.m.views.filter(|v| *v > 0)? as f64 * 100.0)).collect();
        (v.len() >= 2).then(|| median(&mut v).unwrap_or(0.0))
    };
    if let (Some(a), Some(b)) = (rate(top), rate(rest)) {
        out.push_str(&format!(" Interactions per 100 views: {a:.1} for the best, {b:.1} for the rest."));
    }
    if out.is_empty() {
        out.push_str(" The best and the rest don't differ in their openings, length or retention on record, so I can't say why.");
    }
    out
}

// ---------------------------------------------------------------- when to post

const BLOCKS: [(u32, u32, &str); 6] = [(0, 6, "overnight (midnight-6am)"), (6, 9, "early morning (6-9am)"), (9, 12, "morning (9am-noon)"), (12, 15, "early afternoon (noon-3pm)"), (15, 18, "late afternoon (3-6pm)"), (18, 24, "evening (6pm-midnight)")];
const DAYS: [&str; 7] = ["Thursday", "Friday", "Saturday", "Sunday", "Monday", "Tuesday", "Wednesday"];

/// "Which posting times work?" Each post against its platform's median,
/// grouped by when it went out in your time.
pub fn posting_times(book: &Book, only: Option<Platform>, zone: &Zone, min_each: usize) -> String {
    let latest = book.latest_posts();
    let posts: Vec<&PostSnap> = latest.into_iter().filter(|p| only.is_none_or(|o| p.platform == o) && p.posted.is_some()).collect();
    let ranked = against_median(&posts);
    if ranked.len() < min_each * 2 {
        return format!(
            "{} with figures and a posting time on record{} -- too few to say when works. I need at least {} in two different times of day.",
            plural(ranked.len(), "post"),
            only.map(|p| format!(" for {}", p.name())).unwrap_or_default(),
            min_each
        );
    }
    let mut by_block: Vec<(usize, Vec<f64>)> = (0..BLOCKS.len()).map(|i| (i, Vec::new())).collect();
    let mut by_day: Vec<(usize, Vec<f64>)> = (0..7).map(|i| (i, Vec::new())).collect();
    for (p, _, _, r) in &ranked {
        let local = zone.to_local(p.posted.unwrap_or(0) as i64);
        let hour = (local.rem_euclid(86_400) / 3600) as u32;
        if let Some(b) = BLOCKS.iter().position(|(a, z, _)| hour >= *a && hour < *z) {
            by_block[b].1.push(*r);
        }
        by_day[local.div_euclid(86_400).rem_euclid(7) as usize].1.push(*r);
    }
    let summarise = |groups: &mut Vec<(usize, Vec<f64>)>, name: &dyn Fn(usize) -> String| -> (Vec<String>, Vec<String>) {
        let mut ok: Vec<(String, f64, usize)> = Vec::new();
        let mut thin = Vec::new();
        for (i, v) in groups.iter_mut() {
            if v.is_empty() {
                continue;
            }
            if v.len() < min_each {
                thin.push(name(*i));
                continue;
            }
            let n = v.len();
            ok.push((name(*i), median(v).unwrap_or(0.0), n));
        }
        ok.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        (ok.iter().map(|(nm, m, n)| format!("{nm}: {m:.1}x median across {n}")).collect(), thin)
    };
    let (blocks, thin_b) = summarise(&mut by_block, &|i| BLOCKS[i].2.to_string());
    let (days, _) = summarise(&mut by_day, &|i| DAYS[i].to_string());
    let mut s = format!("From {} (each against its platform's median, in your time zone): ", plural(ranked.len(), "post"));
    if blocks.len() >= 2 {
        s.push_str(&format!("best {}; weakest {}.", blocks[0], blocks[blocks.len() - 1]));
    } else if let Some(b) = blocks.first() {
        s.push_str(&format!("only one time of day has enough posts to judge -- {b}."));
    }
    if days.len() >= 2 {
        s.push_str(&format!(" By day: best {}, weakest {}.", days[0], days[days.len() - 1]));
    }
    if !thin_b.is_empty() {
        s.push_str(&format!(" Too few posts to judge {}.", thin_b.join(", ")));
    }
    s.push_str(" This is when your posts did well, not proof that the time caused it.");
    s
}

// ---------------------------------------------------------------- followers

/// "Followers over time", per platform and in total where it can be summed.
pub fn followers(book: &Book, only: Option<Platform>, now: u64) -> String {
    let today = super::social_day(now);
    let mut lines = Vec::new();
    let mut total_change: i64 = 0;
    let mut total_now: u64 = 0;
    let mut summed = Vec::new();
    let mut single = Vec::new();
    let mut none = Vec::new();
    for pf in Platform::ALL.into_iter().filter(|p| only.is_none_or(|o| o == *p)) {
        let days = book.account_days(pf);
        let counted: Vec<(i64, u64)> = days.iter().filter_map(|a| a.followers.map(|f| (a.day, f))).collect();
        if counted.is_empty() {
            // LinkedIn can give new followers a day without a total.
            let gained: i64 = days.iter().filter(|a| a.day > today - 30).filter_map(|a| a.m.followers_gained).sum();
            if days.iter().any(|a| a.m.followers_gained.is_some()) {
                lines.push(format!("{}: +{gained} new followers in the last 30 days on record (no total in the data)", pf.name()));
            } else {
                none.push(pf);
            }
            continue;
        }
        let (last_day, last) = counted[counted.len() - 1];
        let back = |d: i64| counted.iter().rev().find(|(day, _)| *day <= last_day - d).copied();
        let mut line = format!("{}: {} on {}", pf.name(), super::count_said(last), super::day_label(last_day));
        let mut changed = false;
        for (d, word) in [(7, "week"), (30, "30 days")] {
            if let Some((day, then)) = back(d) {
                let diff = last as i64 - then as i64;
                line.push_str(&format!(", {}{diff} over the {word} since {}", if diff >= 0 { "+" } else { "" }, super::day_label(day)));
                if word == "30 days" || back(30).is_none() {
                    total_change += diff;
                    changed = true;
                }
            }
        }
        if counted.len() == 1 {
            line.push_str(" (one day on record, so no change yet)");
            single.push(pf);
        }
        if changed {
            summed.push(pf);
        }
        total_now += last;
        lines.push(line);
    }
    if lines.is_empty() {
        return "I have no follower counts yet. Import a platform's export, or turn on the daily refresh on the Social page -- follower history starts from the first day Atlas sees.".into();
    }
    let mut s = lines.join(". ") + ".";
    if only.is_none() && lines.len() > 1 {
        s.push_str(&format!(" Together: {} followers across the platforms with a total", super::count_said(total_now)));
        if !summed.is_empty() {
            s.push_str(&format!(", {}{total_change} over the time on record for {}", if total_change >= 0 { "+" } else { "" }, name_list(&summed)));
        }
        s.push('.');
    }
    if !none.is_empty() && only.is_none() {
        s.push_str(&format!(" No follower figures from {}.", name_list(&none)));
    }
    s
}

/// What's kept, per platform, and what isn't -- the answer to "what do you
/// have on my accounts?".
pub fn accounts_overview(book: &Book) -> String {
    if book.is_empty() {
        return "Nothing on your accounts yet. Import a platform's own export from the Social page (X archive, TikTok or Instagram data, LinkedIn's analytics .xlsx, YouTube Studio's CSV), or turn on the daily refresh for YouTube, Instagram or Bluesky.".into();
    }
    let latest = book.latest_posts();
    let mut parts = Vec::new();
    for pf in book.platforms() {
        let posts = latest.iter().filter(|p| p.platform == pf).count();
        let days = book.account_days(pf);
        let last = latest.iter().filter(|p| p.platform == pf).map(|p| p.day).chain(days.iter().map(|a| a.day)).max();
        parts.push(format!(
            "{}: {} and {} of account figures, latest {}",
            pf.name(),
            plural(posts, "post"),
            plural(days.len(), "day"),
            last.map(super::day_label).unwrap_or_default()
        ));
    }
    let mut s = format!("{}. ({} records kept.)", parts.join("; "), book.len());
    if let Some(m) = missing_platforms(book, &Platform::ALL) {
        s.push(' ');
        s.push_str(&m);
    }
    if book.unreadable > 0 {
        s.push_str(&format!(" {} lines of the record didn't read and were skipped.", book.unreadable));
    }
    s
}

/// A line for the morning brief about your own accounts: the latest video's
/// first figures, if it went out in the last two days.
pub fn own_brief_line(book: &Book, now: u64) -> Option<String> {
    let posts = book.latest_posts();
    let last = posts.iter().filter(|p| p.platform.is_video() && p.posted.is_some_and(|t| now.saturating_sub(t) < 2 * 86_400)).max_by_key(|p| p.posted)?;
    let (what, v) = headline_figure(last)?;
    Some(format!("Your latest {} video: {} {what} so far", last.platform.name(), super::count_said(v)))
}

// ---------------------------------------------------------------- the people you watch

/// The digest of what you watch, as headed sections of lines, every figure
/// from what was read.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Digest {
    pub sections: Vec<(String, Vec<String>)>,
    /// What couldn't be read, and why.
    pub gaps: Vec<String>,
}

impl Digest {
    /// Everything, as plain text: what a model summary must stay inside.
    pub fn facts(&self) -> String {
        let mut s = String::new();
        for (h, lines) in &self.sections {
            s.push_str(h);
            s.push('\n');
            for l in lines {
                s.push_str("- ");
                s.push_str(l);
                s.push('\n');
            }
        }
        s
    }

    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }
}

pub fn watch_digest(w: &Watch, now: u64) -> Digest {
    let mut d = Digest::default();
    let label_of = |key: &str| w.list.iter().find(|x| x.target.key() == key).map(|x| x.target.label()).unwrap_or_else(|| key.to_string());
    let recent = |s: &&super::watchlist::Seen, days: u64| s.at.is_some_and(|t| now.saturating_sub(t) <= days * 86_400);

    // YouTube channels: views a day since posted, against the channel's own.
    let mut standouts: Vec<(f64, String)> = Vec::new();
    let mut moving: Vec<(f64, String)> = Vec::new();
    for key in w.list.iter().map(|x| x.target.key()).filter(|k| k.starts_with("yt:")) {
        let items: Vec<&super::watchlist::Seen> = w.seen.iter().filter(|s| s.from == key && s.views.is_some() && s.at.is_some()).collect();
        let rate = |s: &super::watchlist::Seen| s.views.unwrap_or(0) as f64 / ((now.saturating_sub(s.at.unwrap_or(now))) as f64 / 86_400.0).max(0.25);
        let mut rates: Vec<f64> = items.iter().map(|s| rate(s)).collect();
        let Some(m) = median(&mut rates) else { continue };
        for s in items.iter().filter(|s| recent(s, 14)) {
            let r = rate(s);
            if items.len() >= 3 && m > 0.0 && r >= 2.0 * m {
                standouts.push((r / m, format!("{} -- \"{}\": {} views in {} days, {:.1}x the channel's usual pace", s.who, s.title, super::count_said(s.views.unwrap_or(0)), (now.saturating_sub(s.at.unwrap_or(now)) / 86_400).max(1), r / m)));
            }
            if let (Some((then, before)), Some(v)) = (s.views_before, s.views) {
                let hours = (s.last_seen.saturating_sub(then)) as f64 / 3600.0;
                if hours >= 0.5 && v > before {
                    moving.push(((v - before) as f64 / hours, format!("{} -- \"{}\": +{} views in the last {:.0} hours", s.who, s.title, super::count_said(v - before), hours)));
                }
            }
        }
    }
    let top = |mut v: Vec<(f64, String)>, n: usize| {
        v.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        v.into_iter().take(n).map(|x| x.1).collect::<Vec<_>>()
    };
    let so = top(standouts, 3);
    if !so.is_empty() {
        d.sections.push(("Doing better than usual on YouTube".into(), so));
    }
    let mv = top(moving, 3);
    if !mv.is_empty() {
        d.sections.push(("Moving fastest since the last read".into(), mv));
    }
    // YouTube topic searches.
    let yts: Vec<(f64, String)> = w
        .seen
        .iter()
        .filter(|s| s.from.starts_with("yts:") && recent(s, 8))
        .filter_map(|s| Some((s.views? as f64, format!("{}: \"{}\" by {} -- {} views", label_of(&s.from).trim_start_matches("YouTube search: "), s.title, s.who, super::count_said(s.views?)))))
        .collect();
    let yts = top(yts, 3);
    if !yts.is_empty() {
        d.sections.push(("Most-viewed this week on your topics".into(), yts));
    }
    // Hacker News.
    let hn: Vec<(f64, String)> = w
        .seen
        .iter()
        .filter(|s| s.from.starts_with("hn:") && recent(s, 2))
        .filter_map(|s| Some((s.points? as f64, format!("\"{}\" -- {} points, {} comments", s.title, s.points?, s.comments.unwrap_or(0)))))
        .collect();
    let hn = top(hn, 5);
    if !hn.is_empty() {
        d.sections.push(("On Hacker News".into(), hn));
    }
    // Mastodon and Bluesky: likes plus reposts.
    for (prefix, head) in [("masto:", "On Mastodon"), ("bsky:", "On Bluesky")] {
        let v: Vec<(f64, String)> = w
            .seen
            .iter()
            .filter(|s| s.from.starts_with(prefix) && recent(s, 3))
            .filter_map(|s| {
                let score = s.likes? + s.reposts.unwrap_or(0);
                (score > 0).then(|| (score as f64, format!("{}: \"{}\" -- {} likes, {} reposts", s.who, s.title.chars().take(90).collect::<String>(), s.likes.unwrap_or(0), s.reposts.unwrap_or(0))))
            })
            .collect();
        let v = top(v, 3);
        if !v.is_empty() {
            d.sections.push((head.into(), v));
        }
    }
    // Trends, Product Hunt, Reddit: listed as given, no ranking invented.
    let listed = |prefix: &str, days: u64, n: usize, f: &dyn Fn(&super::watchlist::Seen) -> String| -> Vec<String> {
        let mut v: Vec<&super::watchlist::Seen> = w.seen.iter().filter(|s| s.from.starts_with(prefix) && recent(s, days)).collect();
        v.sort_by_key(|s| std::cmp::Reverse(s.at));
        v.into_iter().take(n).map(f).collect()
    };
    let tr = listed("trends:", 1, 5, &|s| format!("{} ({} searches)", s.title, s.traffic.clone().unwrap_or_else(|| "traffic not given".into())));
    if !tr.is_empty() {
        d.sections.push(("Trending searches today".into(), tr));
    }
    let ph = listed("ph", 2, 3, &|s| s.title.clone());
    if !ph.is_empty() {
        d.sections.push(("Newest on Product Hunt (the feed has no votes)".into(), ph));
    }
    let rd = listed("reddit:", 2, 5, &|s| format!("{}: {}", label_of(&s.from).split(' ').next().unwrap_or(""), s.title));
    if !rd.is_empty() {
        d.sections.push(("Newest on Reddit (no scores: Reddit's feeds don't carry them)".into(), rd));
    }
    for x in &w.list {
        if !x.last_error.is_empty() {
            d.gaps.push(format!("{}: {}", x.target.label(), x.last_error));
        } else if x.last_ok.is_none() {
            d.gaps.push(format!("{}: not read yet", x.target.label()));
        }
    }
    d
}

/// The morning brief's line on what you watch: the model's summary if it
/// was written in the last day and a half (it was checked against the
/// figures when it came back), otherwise the top line of the first two
/// sections as read -- the same facts, without the prose.
pub fn brief_digest(w: &Watch, now: u64) -> Option<String> {
    if let Some((at, text)) = &w.summary {
        if now.saturating_sub(*at) < 36 * 3600 && !text.trim().is_empty() {
            return Some(text.trim().to_string());
        }
    }
    let d = watch_digest(w, now);
    let lines: Vec<String> = d.sections.iter().take(2).filter_map(|(h, l)| l.first().map(|f| format!("{h}: {f}"))).collect();
    (!lines.is_empty()).then(|| lines.join(". "))
}

// ---------------------------------------------------------------- keeping a model honest

/// The numbers written in a piece of text, as written: "1,234" -> "1234",
/// "1.9M" -> "1.9", "3x" -> "3".
fn numbers_in(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = text.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        let joins = (*c == ',' || *c == '.') && !cur.is_empty() && chars.get(i + 1).is_some_and(|n| n.is_ascii_digit());
        if c.is_ascii_digit() || joins {
            if *c != ',' {
                cur.push(*c);
            }
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out.into_iter().map(|n| if n.contains('.') { n.trim_end_matches('0').trim_end_matches('.').to_string() } else { n }).collect()
}

/// Does every number in `reply` appear in `facts`? A summary that brings a
/// figure of its own is not used; the plain facts are shown instead.
pub fn grounded(reply: &str, facts: &str) -> bool {
    let known = numbers_in(facts);
    numbers_in(reply).iter().all(|n| known.contains(n))
}

/// What the model is told when it summarises the digest.
pub const SUMMARY_SYSTEM: &str = "You summarise what is working for accounts and topics someone watches. \
Use only the facts given. Write three or four plain sentences: what stands out and what it has in common \
(a kind of title, a topic, a format). Every number you write must be one that appears in the facts; \
if unsure, leave numbers out. No lists, no headings.";
