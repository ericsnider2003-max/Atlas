//! Text as it should sound: names, tickers, acronyms and symbols rewritten
//! before they reach the speech engine.
//!
//! **Source:** the idea is the SSML `<sub alias>` / lexicon step every
//! text-to-speech front end has (W3C *Pronunciation Lexicon Specification*
//! 1.0; eSpeak's and piper's own normalisers do the same for numbers). The
//! word lists here are Atlas's own. In house.
//!
//! **Why Atlas wants it.** `docs/GAPS.md` §B, written down and never done:
//! "Local TTS will mangle product names, tickers, and acronyms. A small
//! pronunciation dictionary fixes it and is trivial to build." Piper reads
//! "EURUSD" as one word and "VPS" as "vips"; a reply with "→" or "±" in it is
//! read as silence or as the symbol's Unicode name.

use std::collections::BTreeMap;

/// Built in. Your own entries (`pronounce:` in tools.yaml) win over these.
const NAMES: &[(&str, &str)] = &[
    ("Atlas", "Atlas"),
    ("WireGuard", "Wire guard"),
    ("Ichimoku", "Ichi moku"),
    ("OAuth", "oh auth"),
    ("iCal", "eye cal"),
    ("vCard", "vee card"),
];

/// Acronyms said as a word, not spelled out.
const SAID_AS_WORDS: &[&str] = &["NASA", "ASAP", "PIN", "RAM", "SIM", "GIF", "JPEG", "SQL", "NATO", "OPEC", "FIFO", "LIFO", "AWOL", "SCUBA", "LASER", "RADAR", "CAPTCHA", "OK"];

const CURRENCIES: &[(&str, &str)] = &[
    ("EUR", "euro"),
    ("USD", "dollar"),
    ("GBP", "pound"),
    ("JPY", "yen"),
    ("CHF", "franc"),
    ("AUD", "aussie"),
    ("NZD", "kiwi"),
    ("CAD", "loonie"),
    ("XAU", "gold"),
    ("XAG", "silver"),
];

fn currency(c: &str) -> Option<&'static str> {
    CURRENCIES.iter().find(|(k, _)| *k == c).map(|(_, v)| *v)
}

fn word(w: &str, mine: &BTreeMap<String, String>) -> String {
    // Keep surrounding punctuation where it was.
    let start = w.find(|c: char| c.is_alphanumeric()).unwrap_or(w.len());
    let end = w.rfind(|c: char| c.is_alphanumeric()).map(|i| i + w[i..].chars().next().map_or(1, |c| c.len_utf8())).unwrap_or(start);
    if start >= end {
        return w.to_string();
    }
    let (pre, mut core, mut post) = (&w[..start], &w[start..end], w[end..].to_string());
    // A possessive keeps its 's after the word it belongs to.
    for tail in ["'s", "’s"] {
        if let Some(c) = core.strip_suffix(tail) {
            core = c;
            post = format!("{tail}{post}");
        }
    }
    let post = post.as_str();
    if let Some(s) = mine.get(core).or_else(|| mine.get(&core.to_lowercase())) {
        return format!("{pre}{s}{post}");
    }
    if let Some((_, s)) = NAMES.iter().find(|(k, _)| *k == core) {
        return format!("{pre}{s}{post}");
    }
    // A currency pair: EURUSD, EUR/USD.
    let pair = core.replace('/', "");
    if pair.len() == 6 && pair.chars().all(|c| c.is_ascii_uppercase()) {
        if let (Some(a), Some(b)) = (currency(&pair[..3]), currency(&pair[3..])) {
            return format!("{pre}{a} {b}{post}");
        }
    }
    // An acronym: two to five capitals (a trailing s allowed), spelled out.
    let letters = core.strip_suffix('s').filter(|x| x.len() >= 2).unwrap_or(core);
    if (2..=5).contains(&letters.len()) && letters.chars().all(|c| c.is_ascii_uppercase()) && !SAID_AS_WORDS.contains(&letters) {
        let spelled: Vec<String> = letters.chars().map(|c| c.to_string()).collect();
        let plural = if letters.len() < core.len() { "s" } else { "" };
        return format!("{pre}{}{plural}{post}", spelled.join(" "));
    }
    // 4k, 12k, 3M: a number with a magnitude.
    if let Some(n) = core.strip_suffix(['k', 'K']).filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit() || c == '.')) {
        return format!("{pre}{n} thousand{post}");
    }
    if let Some(n) = core.strip_suffix('M').filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit() || c == '.')) {
        return format!("{pre}{n} million{post}");
    }
    w.to_string()
}

/// `text` as it should be spoken. `mine` is your own list (word → how to say it).
pub fn for_speech(text: &str, mine: &BTreeMap<String, String>) -> String {
    // Numbers, money, times and dates first (`spoken_numbers`), while their
    // own symbols — $, %, the dash in a range — are still attached to them.
    let text = crate::spoken_numbers::words(text);
    // Then the remaining symbols, spaced so they read as words.
    let mut t = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '→' | '⇒' => t.push_str(" to "),
            '←' => t.push_str(" from "),
            '±' => t.push_str(" plus or minus "),
            '≈' | '~' => t.push_str(" about "),
            '×' => t.push_str(" times "),
            '≥' => t.push_str(" at least "),
            '≤' => t.push_str(" at most "),
            '&' => t.push_str(" and "),
            '%' => t.push_str(" percent"),
            '#' => t.push_str(" number "),
            '—' | '–' => t.push_str(", "),
            '·' | '•' => t.push_str(", "),
            _ => t.push(c),
        }
    }
    let spoken: Vec<String> = t
        .split_whitespace()
        .map(|w| {
            if w.starts_with("http://") || w.starts_with("https://") {
                "a link".to_string()
            } else {
                word(w, mine)
            }
        })
        .collect();
    spoken.join(" ").replace(" ,", ",").replace(",,", ",")
}
