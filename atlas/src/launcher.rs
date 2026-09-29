//! One place to say what you want to open: an app, a file, a Start-menu
//! shortcut or one of Atlas's own commands -- ranked by how well the words
//! match and how often, and how lately, you've picked it.
//!
//! **Sources:** PowerToys Run (MIT) and Flow Launcher (MIT), read for the
//! shape: one query box, many plugins, results ranked and the top one
//! taken when it's clearly the one. The ranking is frecency in the sense
//! Mozilla's Places used for the Firefox address bar -- a use counts for less
//! as it ages -- here as an exponential half-life (`HALF_LIFE`), which needs
//! no buckets and stays bounded (`MAX_USES` per item). Typo tolerance is
//! `typos` (OSA distance, the palette's allowance).
//!
//! **Soundproofing.** It opens only what the query clearly means: when the
//! best two are close, it lists them rather than guessing (`Pick::Choose`).
//! It learns only from what you actually picked. The list of things it can
//! open is built from what's on this machine; nothing is fetched.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How long before a use counts half as much.
pub const HALF_LIFE: u64 = 7 * 86_400;
/// Uses remembered per item.
pub const MAX_USES: usize = 20;
/// Items remembered at all.
pub const MAX_ITEMS: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    /// An app from your apps list.
    App,
    /// A Start-menu shortcut.
    Shortcut,
    /// A file from the index.
    File,
    /// One of Atlas's own commands, run as if said.
    Command,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub kind: Kind,
    /// What's shown and matched.
    pub label: String,
    /// How to open it: the app's name, the shortcut's or file's path, the
    /// command's words.
    pub target: String,
}

impl Candidate {
    pub fn key(&self) -> String {
        format!("{:?}:{}", self.kind, self.target.to_lowercase())
    }
}

/// When you picked what.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Uses {
    pub by_key: BTreeMap<String, Vec<u64>>,
}

impl Uses {
    pub fn picked(&mut self, c: &Candidate, now: u64) {
        let v = self.by_key.entry(c.key()).or_default();
        v.push(now);
        let over = v.len().saturating_sub(MAX_USES);
        if over > 0 {
            v.drain(..over);
        }
        if self.by_key.len() > MAX_ITEMS {
            // Forget the item used longest ago.
            if let Some(oldest) = self.by_key.iter().min_by_key(|(_, v)| v.last().copied().unwrap_or(0)).map(|(k, _)| k.clone()) {
                self.by_key.remove(&oldest);
            }
        }
    }

    /// Frecency: each use worth 1, halving every `HALF_LIFE`.
    pub fn weight(&self, c: &Candidate, now: u64) -> f64 {
        self.by_key
            .get(&c.key())
            .map(|v| v.iter().map(|t| 0.5f64.powf(now.saturating_sub(*t) as f64 / HALF_LIFE as f64)).sum())
            .unwrap_or(0.0)
    }
}

/// How well `query` matches `label`, 0 (not at all) to 1 (exactly). The
/// whole label, a word's start, the initials ("vsc" for Visual Studio
/// Code), anywhere inside, then within a typo.
pub fn match_score(query: &str, label: &str) -> f64 {
    let q = query.trim().to_lowercase();
    let l = label.to_lowercase();
    if q.is_empty() {
        return 0.0;
    }
    if l == q {
        return 1.0;
    }
    let words: Vec<&str> = l.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    if l.starts_with(&q) {
        return 0.9;
    }
    if words.iter().any(|w| w.starts_with(&q)) {
        return 0.8;
    }
    let initials: String = words.iter().filter_map(|w| w.chars().next()).collect();
    if q.len() >= 2 && initials.starts_with(&q) {
        return 0.75;
    }
    let qwords: Vec<&str> = q.split_whitespace().collect();
    if qwords.len() > 1 && qwords.iter().all(|qw| words.iter().any(|w| w.starts_with(qw))) {
        return 0.7;
    }
    if l.contains(&q) {
        return 0.6;
    }
    let allow = crate::typos::allowance(q.chars().count());
    if allow > 0 && words.iter().any(|w| crate::typos::osa(w, &q, allow).is_some()) {
        return 0.5;
    }
    0.0
}

/// What to do with a query.
#[derive(Debug, Clone, PartialEq)]
pub enum Pick {
    /// Clearly this one.
    Open(Candidate),
    /// Close between these: say them, numbered.
    Choose(Vec<Candidate>),
    Nothing,
}

/// Ranked results: match first (it must match), frecency to order the
/// matches, a small bonus for apps (usually what's meant by a bare name).
pub fn launch_ranking(query: &str, all: &[Candidate], uses: &Uses, now: u64) -> Vec<(f64, Candidate)> {
    let mut scored: Vec<(f64, Candidate)> = all
        .iter()
        .filter_map(|c| {
            let m = match_score(query, &c.label);
            (m > 0.0).then(|| {
                let kind_bonus = match c.kind {
                    Kind::App => 0.05,
                    Kind::Shortcut => 0.03,
                    Kind::Command => 0.02,
                    Kind::File => 0.0,
                };
                // Frecency can lift a weaker match, never one that doesn't match.
                let f = uses.weight(c, now);
                (m + kind_bonus + 0.1 * (1.0 + f).ln(), c.clone())
            })
        })
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.label.cmp(&b.1.label)));
    // The same thing twice (an app and its own shortcut) is shown once.
    let mut seen: Vec<String> = Vec::new();
    scored.retain(|(_, c)| {
        let k = c.label.to_lowercase();
        if seen.contains(&k) {
            false
        } else {
            seen.push(k);
            true
        }
    });
    scored
}

/// Open the best one only when it's clearly best: a good match, and ahead
/// of the next by a margin.
pub fn pick(query: &str, all: &[Candidate], uses: &Uses, now: u64) -> Pick {
    let ranked = launch_ranking(query, all, uses, now);
    match ranked.as_slice() {
        [] => Pick::Nothing,
        [(s, c)] if *s >= 0.5 => Pick::Open(c.clone()),
        [(s1, c1), (s2, _), ..] if *s1 >= 0.75 && s1 - s2 >= 0.1 => Pick::Open(c1.clone()),
        _ => Pick::Choose(ranked.into_iter().take(5).map(|(_, c)| c).collect()),
    }
}

/// Start-menu shortcuts under `dirs`: the `.lnk` files, named by their file
/// name. Walked to a fixed depth, so a huge tree costs a bounded amount.
pub fn shortcuts(dirs: &[std::path::PathBuf]) -> Vec<Candidate> {
    fn walk(dir: &std::path::Path, depth: u32, out: &mut Vec<Candidate>) {
        if depth > 4 || out.len() > 2000 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, depth + 1, out);
            } else if p.extension().map(|x| x.eq_ignore_ascii_case("lnk")).unwrap_or(false) {
                let name = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                // Uninstallers and help files are never what's meant.
                let low = name.to_lowercase();
                if low.contains("uninstall") || low.contains("readme") || low.ends_with(" help") {
                    continue;
                }
                out.push(Candidate { kind: Kind::Shortcut, label: name, target: p.display().to_string() });
            }
        }
    }
    let mut out = Vec::new();
    for d in dirs {
        walk(d, 0, &mut out);
    }
    out
}

/// Where Windows keeps Start-menu shortcuts: all users' and yours.
pub fn start_menu_dirs() -> Vec<std::path::PathBuf> {
    let mut v = Vec::new();
    if let Ok(p) = std::env::var("ProgramData") {
        v.push(std::path::Path::new(&p).join("Microsoft/Windows/Start Menu/Programs"));
    }
    if let Ok(p) = std::env::var("APPDATA") {
        v.push(std::path::Path::new(&p).join("Microsoft/Windows/Start Menu/Programs"));
    }
    v
}

/// The choice, said: "1. Chrome (app) 2. Chrome Remote Desktop (shortcut)".
pub fn say_choices(list: &[Candidate]) -> String {
    let parts: Vec<String> = list
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let kind = match c.kind {
                Kind::App => "app",
                Kind::Shortcut => "shortcut",
                Kind::File => "file",
                Kind::Command => "Atlas",
            };
            format!("{}. {} ({kind})", i + 1, c.label)
        })
        .collect();
    format!("Which one? {} -- say \"launch 1\" or the name more fully.", parts.join("  "))
}
