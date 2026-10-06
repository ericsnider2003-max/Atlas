//! Finding a file by what you remember of it: part of its name, what kind of
//! thing it is, roughly when you last touched it -- and then opening it.
//!
//! Built on the index (`index::Index`), which already knows every file under
//! your roots by name, type and date. What this adds:
//!
//! - **Filters from the words.** "the pdf from last week", "spreadsheets
//!   from yesterday", "photos this month": a type and a date window are read
//!   out of the question and the rest is the name.
//! - **Paths, numbered.** "open 2" opens the second one (`Found`), with the
//!   app Windows uses for that type.
//! - **A near miss is offered, never taken.** The index's search is exact on
//!   purpose ("a wrong file confidently returned is worse than no result");
//!   when it finds nothing, names within a typo are listed as "closest",
//!   so you can say which -- not opened.
//!
//! **Sources:** Everything (voidtools) and Recoll were read for what makes a
//! finder feel instant -- a filename index held in memory and filters that
//! narrow before ranking -- which is the shape `index` already has.

use crate::index::{AssetClass, Entry};

/// What the words asked for, beyond the name.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Asked {
    pub words: Vec<String>,
    /// Allowed extensions, lower-case, none meaning any.
    pub exts: Vec<&'static str>,
    pub class: Option<AssetClass>,
    /// Modified within [from, to), UTC seconds.
    pub window: Option<(u64, u64)>,
}

const TYPES: &[(&[&str], &[&str], Option<AssetClass>)] = &[
    (&["pdf", "pdfs"], &["pdf"], None),
    (&["spreadsheet", "spreadsheets", "excel", "sheet", "sheets", "csv"], &["xlsx", "xls", "xlsm", "csv", "ods"], None),
    (&["doc", "docs", "word", "document", "documents"], &["docx", "doc", "odt", "rtf", "md", "txt", "pdf"], None),
    (&["slides", "presentation", "deck", "powerpoint"], &["pptx", "ppt", "odp", "key"], None),
    (&["photo", "photos", "picture", "pictures", "image", "images", "screenshot", "screenshots"], &[], Some(AssetClass::Image)),
    (&["video", "videos", "recording", "recordings"], &[], Some(AssetClass::Video)),
    (&["zip", "archive", "archives"], &[], Some(AssetClass::Archive)),
];

const FILLER: &[&str] = &[
    "the", "a", "an", "my", "file", "files", "find", "where", "is", "was", "that", "from", "of", "called",
    "named", "about", "open", "which", "i", "saved", "made", "with", "in", "on", "for",
];

/// Read the question. `now` is UTC seconds; `day_start` is the start of
/// your local day, in UTC seconds, so "today" is your today.
pub fn read(question: &str, now: u64, day_start: u64) -> Asked {
    let low = question.to_lowercase();
    let mut a = Asked::default();
    let (window, used): (Option<(u64, u64)>, &[&str]) = if low.contains("yesterday") {
        (Some((day_start.saturating_sub(86_400), day_start)), &["yesterday"])
    } else if low.contains("today") {
        (Some((day_start, now + 1)), &["today"])
    } else if low.contains("last week") {
        (Some((day_start.saturating_sub(14 * 86_400), day_start.saturating_sub(7 * 86_400))), &["last", "week"])
    } else if low.contains("this week") {
        (Some((day_start.saturating_sub(7 * 86_400), now + 1)), &["this", "week"])
    } else if low.contains("this month") {
        (Some((day_start.saturating_sub(31 * 86_400), now + 1)), &["this", "month"])
    } else if low.contains("last month") {
        (Some((day_start.saturating_sub(62 * 86_400), day_start.saturating_sub(31 * 86_400))), &["last", "month"])
    } else {
        (None, &[])
    };
    a.window = window;
    for w in low.split(|c: char| !c.is_alphanumeric() && c != '.' && c != '-' && c != '_') {
        if w.is_empty() || FILLER.contains(&w) || used.contains(&w) {
            continue;
        }
        if let Some((_, exts, class)) = TYPES.iter().find(|(names, _, _)| names.contains(&w)) {
            a.exts.extend_from_slice(exts);
            if class.is_some() {
                a.class = *class;
            }
            continue;
        }
        a.words.push(w.to_string());
    }
    a
}

/// Does this entry pass the type and date filters?
pub fn passes(e: &Entry, a: &Asked) -> bool {
    let ext = e.ext.to_lowercase();
    let type_ok = match (&a.exts.is_empty(), a.class) {
        (true, None) => true,
        (_, Some(c)) if e.class == c => true,
        (false, _) => a.exts.contains(&ext.as_str()),
        _ => false,
    };
    let date_ok = a.window.map(|(f, t)| e.modified >= f && e.modified < t).unwrap_or(true);
    type_ok && date_ok
}

/// With no name words, a filter alone ("the pdfs from yesterday") lists
/// what passes, newest first.
pub fn by_filter<'a>(entries: impl Iterator<Item = &'a Entry>, a: &Asked, n: usize) -> Vec<&'a Entry> {
    let mut v: Vec<&Entry> = entries.filter(|e| passes(e, a)).collect();
    v.sort_by_key(|y| std::cmp::Reverse(y.modified));
    v.truncate(n);
    v
}

/// Names within a typo of the words, for "closest" -- offered, never opened.
pub fn near<'a>(entries: impl Iterator<Item = &'a Entry>, a: &Asked, n: usize) -> Vec<&'a Entry> {
    let mut scored: Vec<(usize, &Entry)> = entries
        .filter(|e| passes(e, a))
        .filter_map(|e| {
            let stem = e.name.rsplit_once('.').map(|(s, _)| s).unwrap_or(&e.name).to_lowercase();
            let tokens: Vec<&str> = stem.split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()).collect();
            let mut total = 0;
            for w in &a.words {
                let allow = crate::typos::allowance(w.chars().count()).max(1);
                let best = tokens.iter().filter_map(|t| crate::typos::osa(t, w, allow)).min()?;
                total += best;
            }
            (!a.words.is_empty()).then_some((total, e))
        })
        .collect();
    scored.sort_by(|x, y| x.0.cmp(&y.0).then(y.1.modified.cmp(&x.1.modified)));
    scored.into_iter().take(n).map(|(_, e)| e).collect()
}

/// A path shortened for saying: the last two folders and the name.
pub fn short_path(path: &str) -> String {
    let parts: Vec<&str> = path.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    if parts.len() <= 3 {
        return parts.join("/");
    }
    format!("…/{}", parts[parts.len() - 3..].join("/"))
}

/// The list, numbered for "open 2".
pub fn numbered(paths: &[String]) -> String {
    paths.iter().enumerate().map(|(i, p)| format!("{}. {}", i + 1, short_path(p))).collect::<Vec<_>>().join("  ")
}

/// "open 2", "open the second one", "open it": which of the last list.
pub fn which(said: &str, count: usize) -> Option<usize> {
    let low = said.to_lowercase();
    let words: Vec<&str> = low.split_whitespace().collect();
    if count == 0 || words.first().map(|w| *w != "open").unwrap_or(true) {
        return None;
    }
    const ORD: &[(&str, usize)] = &[("first", 1), ("second", 2), ("third", 3), ("fourth", 4), ("fifth", 5), ("last", usize::MAX)];
    for w in &words[1..] {
        let w = w.trim_matches(|c: char| !c.is_alphanumeric());
        if let Ok(n) = w.parse::<usize>() {
            return (1..=count).contains(&n).then_some(n - 1);
        }
        if let Some((_, n)) = ORD.iter().find(|(o, _)| *o == w) {
            return if *n == usize::MAX { Some(count - 1) } else { (*n <= count).then(|| *n - 1) };
        }
    }
    (words.len() == 2 && matches!(words[1], "it" | "that" | "one") && count == 1).then_some(0)
}
