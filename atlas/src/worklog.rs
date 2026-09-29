//! Where the time went — kept by Atlas as you work, on this machine only.
//!
//! Every tick Atlas already looks at which window has the foreground. This
//! keeps that as a record: which app, which window, for how long — and, from
//! the keyboard and mouse, whether you were at the machine at all. From it:
//! "where did my time go today?", your longest stretch of focus, how often you
//! switched, and — when you come back from a break — what you were in the
//! middle of.
//!
//! **How it's kept.** ActivityWatch's model (`ActivityWatch/activitywatch`,
//! MPL-2.0; its data model and heartbeat documentation read as the reference,
//! clean-room): a watcher sends a *heartbeat* of the current state, and a
//! heartbeat identical to the last span that arrives within the *pulse time*
//! extends that span instead of starting a new one — so a day is a few
//! hundred spans, not tens of thousands of samples. Away time is keyboard and
//! mouse silence of `away_after` seconds (ActivityWatch's AFK watcher uses
//! three minutes); the span ends when the input stopped, not when Atlas
//! noticed. Where the platform can't say how long since the last input,
//! away time can't be told from working time, and the report says so.
//!
//! **Focus.** A focus block is a run of time in one category that tolerates
//! short excursions (a glance at mail under a minute) and breaks nothing
//! shorter than `lull` seconds away from the keyboard; it counts from 25
//! minutes (a choice, the length of a Pomodoro, not a finding). Switches are
//! changes of category that last at least ten seconds.
//!
//! **Privacy.** Nothing here leaves the machine or goes into sync. Window
//! titles pass through `redact` first, so a key or card number in a title
//! never lands on disk, and titles can be switched off (`keep_titles`).

use crate::platform::ActiveWindow;
use serde::{Deserialize, Serialize};

/// One stretch of time in one window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub start: u64,
    pub end: u64,
    pub app: String,
    #[serde(default)]
    pub title: String,
    pub category: String,
}

impl Span {
    pub fn secs(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }
}

/// A rule: an app or title containing any of `matches` is in `category`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub category: String,
    pub matches: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkLogConfig {
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Keyboard and mouse silence that counts as away.
    #[serde(default = "d_away")]
    pub away_after: u64,
    /// A heartbeat this soon after a span ends, for the same window, extends it.
    #[serde(default = "d_pulse")]
    pub pulse_secs: u64,
    #[serde(default = "yes")]
    pub keep_titles: bool,
    /// Days kept on disk.
    #[serde(default = "d_keep")]
    pub keep_days: u64,
    /// Your own categories, checked before the built-in ones.
    #[serde(default)]
    pub categories: Vec<Rule>,
    /// Coming back from a break, say what you were in the middle of.
    #[serde(default = "yes")]
    pub resume_cue: bool,
}

fn yes() -> bool {
    true
}
fn d_away() -> u64 {
    180
}
fn d_pulse() -> u64 {
    90
}
fn d_keep() -> u64 {
    35
}

impl Default for WorkLogConfig {
    fn default() -> Self {
        WorkLogConfig { enabled: true, away_after: d_away(), pulse_secs: d_pulse(), keep_titles: true, keep_days: d_keep(), categories: Vec::new(), resume_cue: true }
    }
}

/// The built-in categories: common Windows programs and the words their
/// titles carry. Yours come first and win.
fn built_in() -> Vec<Rule> {
    let r = |c: &str, m: &[&str]| Rule { category: c.into(), matches: m.iter().map(|s| s.to_string()).collect() };
    vec![
        r("trading", &["terminal64", "metatrader", "tradingview", "ninjatrader", "thinkorswim", "ctrader"]),
        r("meetings", &["zoom", "webex", "meet.google", "google meet", "teams meeting", "| microsoft teams call", "gotomeeting"]),
        r("chat", &["slack", "discord", "whatsapp", "telegram", "signal", "ms-teams", "teams.exe"]),
        r("mail", &["outlook", "thunderbird", "gmail", "- mail", "inbox", "mailbird", "hxoutlook"]),
        r("coding", &["code.exe", "visual studio", "devenv", "rider", "idea64", "pycharm", "sublime", "notepad++", "nvim", "vim", "windowsterminal", "powershell", "cmd.exe", "wezterm", "alacritty", "github", "gitlab", "cargo"]),
        r("writing", &["winword", "microsoft word", "notion", "obsidian", "onenote", "google docs", "typora", "notepad"]),
        r("sheets", &["excel", "google sheets", "libreoffice calc"]),
        r("design", &["figma", "photoshop", "illustrator", "blender", "gimp", "canva", "davinci", "premiere", "resolve"]),
        r("video", &["youtube", "netflix", "twitch", "vlc", "prime video"]),
        r("files", &["explorer.exe", "file explorer"]),
        r("browsing", &["chrome", "msedge", "firefox", "brave", "opera", "vivaldi"]),
    ]
}

/// Which category a window belongs to: your rules, then the built-ins, then
/// the app's own name.
pub fn category_for(app: &str, title: &str, rules: &[Rule]) -> String {
    let hay = format!("{} {}", app.to_lowercase(), title.to_lowercase());
    for r in rules.iter().chain(built_in().iter()) {
        if r.matches.iter().any(|m| !m.is_empty() && hay.contains(&m.to_lowercase())) {
            return r.category.clone();
        }
    }
    let a = app.to_lowercase();
    a.strip_suffix(".exe").unwrap_or(&a).to_string()
}

/// The record, with the span still growing at its end.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkLog {
    pub spans: Vec<Span>,
    /// False once any heartbeat came without input timing: away time then
    /// can't be told from working time.
    #[serde(default)]
    pub saw_input: bool,
    #[serde(default)]
    pub blind_beats: u64,
    /// When the record last left memory for disk.
    #[serde(skip)]
    pub saved_at: u64,
    /// Pauses in your input, per app, as a count per second of length
    /// (index 0 is unused): what an ordinary pause looks like in each of
    /// your apps, so a break can be told from a breath (`pause_thresholds`).
    #[serde(default)]
    pub pauses: std::collections::BTreeMap<String, Vec<u32>>,
    /// The last (app, time, input idle) seen, to find where a pause ended.
    #[serde(skip)]
    pause_seen: Option<(String, u64, u64)>,
}

/// Pauses longer than this aren't pauses; they're away.
pub const PAUSE_BINS: usize = 180;
/// How many pauses an app needs before its own threshold is trusted.
pub const PAUSES_TO_LEARN: u32 = 80;

/// What a heartbeat found.
#[derive(Debug, Clone, PartialEq)]
pub enum Beat {
    /// Working in this window.
    Active,
    /// Away since `since` (the last input).
    Away { since: u64 },
    /// No window, or switched off.
    Nothing,
}

impl WorkLog {
    /// One look at the machine: the focused window and seconds since the last
    /// keyboard or mouse input (`None` when the platform can't say).
    pub fn beat(&mut self, cfg: &WorkLogConfig, t: u64, active: Option<&ActiveWindow>, input_idle: Option<u64>) -> Beat {
        if !cfg.enabled {
            return Beat::Nothing;
        }
        // The clock went backwards (a correction, a wrong time set by hand):
        // wait for it to pass where the record already reaches rather than
        // start a span that ends before it began.
        if self.spans.last().map(|l| t < l.end).unwrap_or(false) {
            return Beat::Nothing;
        }
        match input_idle {
            Some(_) => self.saw_input = true,
            None => self.blind_beats += 1,
        }
        if let Some(idle) = input_idle {
            if idle >= cfg.away_after {
                // Away: the span ends at the last input, not now.
                let since = t.saturating_sub(idle);
                if let Some(last) = self.spans.last_mut() {
                    if last.end > since && last.start <= since {
                        last.end = since;
                    }
                }
                return Beat::Away { since };
            }
        }
        let Some(w) = active else { return Beat::Nothing };
        if w.process.is_empty() && w.title.is_empty() {
            return Beat::Nothing;
        }
        let title = if cfg.keep_titles { clean_title(&w.title) } else { String::new() };
        let category = category_for(&w.process, &w.title, &cfg.categories);
        if let Some(last) = self.spans.last_mut() {
            let fresh = t.saturating_sub(last.end) <= cfg.pulse_secs && t >= last.end;
            if fresh && last.app == w.process && last.title == title {
                last.end = t;
                return Beat::Active;
            }
            // A new window straight after the last: the new span starts
            // where that one ended, so the day adds up to the time it took.
            if fresh {
                let start = last.end;
                self.spans.push(Span { start, end: t, app: w.process.clone(), title, category });
                return Beat::Active;
            }
        }
        self.spans.push(Span { start: t, end: t, app: w.process.clone(), title, category });
        Beat::Active
    }
}

/// A call or a video holds your attention without a key being pressed: a
/// 40-minute meeting is not 37 minutes away. So while the window in front
/// is a meeting or a video, or Windows says a full-screen app or a
/// presentation has the screen, input silence shorter than `WATCHING_MAX`
/// doesn't count; past it (asleep in front of a film), it does again.
pub fn effective_idle(
    cfg: &WorkLogConfig,
    active: Option<&ActiveWindow>,
    os_quiet: Option<crate::platform::OsQuiet>,
    idle: Option<u64>,
) -> Option<u64> {
    let idle = idle?;
    let on_screen = active
        .map(|w| matches!(category_for(&w.process, &w.title, &cfg.categories).as_str(), "meetings" | "video"))
        .unwrap_or(false);
    let full = matches!(
        os_quiet,
        Some(crate::platform::OsQuiet::FullScreen | crate::platform::OsQuiet::Presenting | crate::platform::OsQuiet::Game)
    );
    Some(if (on_screen || full) && idle < WATCHING_MAX { 0 } else { idle })
}

/// The longest silence a call or a video explains: two hours.
pub const WATCHING_MAX: u64 = 2 * 3600;

impl WorkLog {
    /// Watch the input's idle count for a pause ending in `app`, and keep its
    /// length. A pause is the time between the last input before it and the
    /// first after, so it's worked out from two readings, not the tick.
    pub fn note_pause(&mut self, app: &str, t: u64, idle: Option<u64>) {
        let Some(idle) = idle else {
            self.pause_seen = None;
            return;
        };
        if let Some((a, pt, pidle)) = self.pause_seen.take() {
            if a == app && idle < pidle && pidle >= 2 && t >= pt {
                let pause = (t - idle).saturating_sub(pt.saturating_sub(pidle));
                if (2..PAUSE_BINS as u64).contains(&pause) {
                    let h = self.pauses.entry(app.to_string()).or_insert_with(|| vec![0; PAUSE_BINS]);
                    h[pause as usize] = h[pause as usize].saturating_add(1);
                }
            }
        }
        self.pause_seen = Some((app.to_string(), t, idle));
    }

    /// Per app, the pause that counts as a natural break: longer than nine
    /// in ten of that app's own pauses, kept between 8 s and 90 s. An app
    /// with fewer than `PAUSES_TO_LEARN` pauses on record has none here and
    /// the fixed 20 s stands. (Iqbal & Bailey found breakpoints are the
    /// boundaries *between* sub-tasks; a pause longer than your usual ones in
    /// that app is the cheapest sign of one.)
    pub fn pause_thresholds(&self) -> std::collections::BTreeMap<String, u64> {
        let mut out = std::collections::BTreeMap::new();
        for (app, h) in &self.pauses {
            let total: u32 = h.iter().sum();
            if total < PAUSES_TO_LEARN {
                continue;
            }
            let want = (total as f64 * 0.9).ceil() as u32;
            let mut run = 0;
            for (secs, n) in h.iter().enumerate() {
                run += n;
                if run >= want {
                    out.insert(app.clone(), (secs as u64 + 1).clamp(8, 90));
                    break;
                }
            }
        }
        out
    }

    /// Drop spans older than `keep_days` before `t`.
    pub fn prune(&mut self, cfg: &WorkLogConfig, t: u64) {
        let cut = t.saturating_sub(cfg.keep_days * 86_400);
        self.spans.retain(|s| s.end >= cut);
    }

    /// The spans overlapping `[from, to)`, clipped to it.
    pub fn between(&self, from: u64, to: u64) -> Vec<Span> {
        self.spans
            .iter()
            .filter(|s| s.end > from && s.start < to)
            .map(|s| Span { start: s.start.max(from), end: s.end.min(to), ..s.clone() })
            .filter(|s| s.secs() > 0)
            .collect()
    }

    /// What you were in the middle of before `before`: the last category you
    /// spent at least a minute in, how long that stretch ran, and its last
    /// window — the cue for picking back up.
    pub fn last_context(&self, before: u64) -> Option<Context> {
        let spans: Vec<&Span> = self.spans.iter().filter(|s| s.start < before).collect();
        let last = spans.iter().rev().find(|s| s.secs() >= 60)?;
        // Walk back through that category, allowing short excursions.
        let mut start = last.start;
        let mut outside = 0u64;
        for s in spans.iter().rev() {
            if s.end > last.end {
                continue;
            }
            if s.category == last.category {
                if start.saturating_sub(s.end) > 300 {
                    break;
                }
                start = s.start;
                outside = 0;
            } else {
                outside += s.secs();
                if outside > 120 {
                    break;
                }
            }
        }
        Some(Context { category: last.category.clone(), app: last.app.clone(), title: last.title.clone(), since: start, until: last.end.min(before) })
    }
}

/// Titles are for you to recognise, not for storing secrets: anything that
/// looks like a key, token, card or account number is replaced, and long
/// titles are cut.
fn clean_title(t: &str) -> String {
    let scrubbed = crate::redact::Scrubber::default().scrub(t);
    let mut out: String = scrubbed.chars().take(120).collect();
    if scrubbed.chars().count() > 120 {
        out.push('…');
    }
    out
}

/// What you were in the middle of.
#[derive(Debug, Clone, PartialEq)]
pub struct Context {
    pub category: String,
    pub app: String,
    pub title: String,
    pub since: u64,
    pub until: u64,
}

impl Context {
    /// "You'd been in coding for 40 minutes — last in main.rs – Visual Studio Code."
    pub fn cue(&self) -> String {
        let how_long = duration_words(self.until.saturating_sub(self.since));
        if self.title.is_empty() {
            format!("Before the break you'd been in {} for {how_long}, in {}.", self.category, self.app)
        } else {
            format!("Before the break you'd been in {} for {how_long} — last in \"{}\".", self.category, self.title)
        }
    }
}

/// A focus block: one category, held.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub category: String,
    pub start: u64,
    pub end: u64,
}

/// A summary of some stretch of time.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub active: u64,
    /// Seconds per category, most first.
    pub by_category: Vec<(String, u64)>,
    /// Seconds per app, most first.
    pub by_app: Vec<(String, u64)>,
    /// Focus blocks of 25 minutes or more, longest first.
    pub blocks: Vec<Block>,
    /// Changes of category lasting at least ten seconds.
    pub switches: usize,
    pub first: Option<u64>,
    pub last: Option<u64>,
}

/// The shortest run that counts as a focus block.
pub const FOCUS_MIN: u64 = 25 * 60;
/// Longest excursion into something else that doesn't break focus.
const EXCURSION: u64 = 60;
/// Longest gap in activity that doesn't break focus.
const LULL: u64 = 5 * 60;

pub fn summarise(spans: &[Span]) -> Summary {
    let mut cat: Vec<(String, u64)> = Vec::new();
    let mut app: Vec<(String, u64)> = Vec::new();
    let add = |v: &mut Vec<(String, u64)>, k: &str, n: u64| match v.iter_mut().find(|(x, _)| x == k) {
        Some(e) => e.1 += n,
        None => v.push((k.to_string(), n)),
    };
    for s in spans {
        add(&mut cat, &s.category, s.secs());
        add(&mut app, &s.app, s.secs());
    }
    cat.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    app.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    // Switches: category changes that last at least ten seconds.
    let mut switches = 0;
    let mut current: Option<&str> = None;
    for s in spans.iter().filter(|s| s.secs() >= 10) {
        if current.is_some() && current != Some(s.category.as_str()) {
            switches += 1;
        }
        current = Some(&s.category);
    }

    // Focus blocks.
    let mut blocks: Vec<Block> = Vec::new();
    let mut open: Option<Block> = None;
    let mut away_from_it = 0u64;
    for s in spans {
        match &mut open {
            Some(b) if s.category == b.category => {
                if s.start.saturating_sub(b.end) > LULL {
                    let done = std::mem::replace(b, Block { category: s.category.clone(), start: s.start, end: s.end });
                    blocks.push(done);
                } else {
                    b.end = s.end;
                }
                away_from_it = 0;
            }
            Some(b) => {
                away_from_it += s.secs();
                if away_from_it > EXCURSION || s.start.saturating_sub(b.end) > LULL {
                    blocks.push(open.take().unwrap_or_else(|| unreachable!()));
                    open = Some(Block { category: s.category.clone(), start: s.start, end: s.end });
                    away_from_it = 0;
                }
            }
            None => open = Some(Block { category: s.category.clone(), start: s.start, end: s.end }),
        }
    }
    blocks.extend(open);
    blocks.retain(|b| b.end.saturating_sub(b.start) >= FOCUS_MIN);
    blocks.sort_by(|a, b| b.end.saturating_sub(b.start).cmp(&a.end.saturating_sub(a.start)).then(a.start.cmp(&b.start)));

    Summary {
        active: spans.iter().map(Span::secs).sum(),
        by_category: cat,
        by_app: app,
        blocks,
        switches,
        first: spans.first().map(|s| s.start),
        last: spans.last().map(|s| s.end),
    }
}

/// "3 h 20 min", "45 min", "under a minute".
pub fn duration_words(secs: u64) -> String {
    let mins = (secs + 30) / 60;
    match mins {
        0 => "under a minute".into(),
        1..=59 => format!("{mins} min"),
        _ if mins % 60 == 0 => format!("{} h", mins / 60),
        _ => format!("{} h {} min", mins / 60, mins % 60),
    }
}

/// The summary in a few sentences. `clock` turns a UTC second into "9:40".
pub fn say(s: &Summary, span_name: &str, blind: bool, clock: &dyn Fn(u64) -> String) -> String {
    if s.active == 0 {
        return format!("I have no record of you at the machine {span_name}.");
    }
    let mut out = format!("{} at the machine {span_name}", duration_words(s.active));
    let top: Vec<String> = s.by_category.iter().take(4).map(|(c, n)| format!("{c} {}", duration_words(*n))).collect();
    out.push_str(&format!(": {}.", top.join(", ")));
    match s.blocks.first() {
        Some(b) => out.push_str(&format!(
            " Longest stretch of focus: {} on {} from {}.",
            duration_words(b.end.saturating_sub(b.start)),
            b.category,
            clock(b.start)
        )),
        None => out.push_str(" No stretch of 25 minutes on one thing."),
    }
    let hours = s.active as f64 / 3600.0;
    if s.switches == 0 {
        out.push_str(" You stayed on the one thing.");
    } else if hours >= 0.5 {
        out.push_str(&format!(" You switched between things about {:.0} times an hour.", (s.switches as f64 / hours).max(1.0)));
    }
    if blind {
        out.push_str(" (This machine doesn't tell me when you're away from the keyboard, so breaks may be counted in.)");
    }
    out
}

/// "How long was I on YouTube?": the time on one thing named in `asked` --
/// a category ("trading", "video"), an app ("excel"), or a word in the
/// window titles ("metatrader") -- or `None` when nothing in the record
/// answers to any of its words, so the caller gives the whole report.
pub fn time_on(spans: &[Span], asked: &str, span_name: &str) -> Option<String> {
    const SKIP: &[&str] = &[
        "how", "long", "much", "time", "was", "were", "did", "spend", "spent", "have", "been", "the", "and",
        "for", "today", "yesterday", "this", "week", "last", "what", "where", "doing", "with", "into", "about",
    ];
    let words: Vec<String> = asked
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3 && !SKIP.contains(w))
        .map(|w| w.to_string())
        .collect();
    if words.is_empty() {
        return None;
    }
    let hit = |sp: &Span| {
        let (c, a, t) = (sp.category.to_lowercase(), sp.app.to_lowercase(), sp.title.to_lowercase());
        words.iter().any(|w| c == *w || a.contains(w.as_str()) || t.contains(w.as_str()))
    };
    let mut total = 0;
    let mut apps: Vec<(String, u64)> = Vec::new();
    for sp in spans.iter().filter(|sp| hit(sp)) {
        let d = sp.end.saturating_sub(sp.start);
        total += d;
        match apps.iter_mut().find(|(a, _)| *a == sp.app) {
            Some(x) => x.1 += d,
            None => apps.push((sp.app.clone(), d)),
        }
    }
    let named = words.join(" ");
    if total == 0 {
        return Some(format!("Nothing I recorded {span_name} matches \"{named}\"."));
    }
    apps.sort_by(|a, b| b.1.cmp(&a.1));
    let detail: Vec<String> = apps.iter().take(3).map(|(a, d)| format!("{a} {}", duration_words(*d))).collect();
    Some(format!("{} on \"{named}\" {span_name} ({}).", duration_words(total), detail.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(app: &str, title: &str) -> ActiveWindow {
        ActiveWindow { process: app.into(), title: title.into() }
    }

    #[test]
    fn heartbeats_in_one_window_make_one_span_and_away_ends_it_at_the_last_input() {
        let cfg = WorkLogConfig::default();
        let mut log = WorkLog::default();
        for t in (0..=700).step_by(2) {
            log.beat(&cfg, 1000 + t, Some(&w("Code.exe", "main.rs")), Some(1));
        }
        assert_eq!(log.spans.len(), 1);
        // Input stops at 1700; Atlas notices at 1900 (200 s idle).
        assert_eq!(log.beat(&cfg, 1900, Some(&w("Code.exe", "main.rs")), Some(200)), Beat::Away { since: 1700 });
        assert_eq!(log.spans[0].end, 1700);
        assert_eq!(log.spans[0].category, "coding");
    }

    #[test]
    fn a_secret_in_a_title_never_reaches_the_record() {
        let cfg = WorkLogConfig::default();
        let mut log = WorkLog::default();
        log.beat(&cfg, 10, Some(&w("notepad.exe", "key AKIAIOSFODNN7EXAMPLE - Notepad")), Some(0));
        assert!(!log.spans[0].title.contains("AKIAIOSFODNN7EXAMPLE"), "{}", log.spans[0].title);
    }
}
