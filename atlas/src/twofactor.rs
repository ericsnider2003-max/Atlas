//! Two-factor codes: Atlas typing them in for you.
//!
//! Eric, 25 Sep 2026 (B1): "I want to be able to use Atlas for two factor",
//! options 1 and 2. Either you read the code out and Atlas types it, or
//! Atlas finds it in your email or your texts and types it. Turning
//! two-factor on or off is the other half, and it lives in `confirmed`
//! (read-back, then your yes, with you at the machine).
//!
//! What this file decides is the part that can go quietly wrong:
//!
//! - **Which number is the code.** A security email has a lot of numbers
//!   in it: the year, a phone number, an order number, a price. A code sits
//!   next to words like "code" or "verification", it is 4 to 8 digits, and it
//!   is not a year, a price or a percentage. When more than one fits, the one
//!   nearest those words wins.
//! - **Which message is the right one.** Only codes from the last few minutes
//!   count (they expire anyway), and one from the site you're signing in to
//!   beats a newer one from somewhere else.
//! - **Where it goes.** Into the code box on the page, including the kind
//!   that is six separate one-digit boxes. Never into a password box.
//!
//! Nothing here sends a code anywhere but the box it was asked for.

use serde::{Deserialize, Serialize};

/// A code found in a message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Found {
    pub code: String,
    /// Who it came from (sender address or the texting number/name).
    pub from: String,
    /// When it arrived, seconds since the epoch. 0 when unknown.
    pub at: u64,
}

/// Where a code is to be typed, captured when you asked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Target {
    /// The code box on the page in Atlas's own browser, mid sign-in.
    AtlasBrowser { site: String },
    /// Whatever window was in front when you asked (your own browser, an app).
    Window { win: u64, title: String },
}

/// Where you said the code is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// You read it out: the digits are in what you said.
    Spoken,
    Email,
    Texts,
}

const WORD_DIGITS: &[(&str, char)] = &[
    ("zero", '0'), ("oh", '0'), ("o", '0'), ("nought", '0'),
    ("one", '1'), ("won", '1'),
    ("two", '2'), ("to", '2'), ("too", '2'),
    ("three", '3'), ("tree", '3'),
    ("four", '4'), ("for", '4'), ("fore", '4'),
    ("five", '5'),
    ("six", '6'),
    ("seven", '7'),
    ("eight", '8'), ("ate", '8'),
    ("nine", '9'), ("niner", '9'),
];

/// Where you said the code is, from your own words.
///
/// "it's in my email" / "from my texts" / "the code is 4 8 2 9 1 7".
pub fn source_of(said: &str) -> Source {
    let t = said.to_lowercase();
    if ["email", "e-mail", "mail", "inbox", "gmail", "outlook"].iter().any(|w| t.contains(w)) {
        Source::Email
    } else if ["text", "sms", "messages", "phone"].iter().any(|w| t.contains(w)) {
        Source::Texts
    } else {
        Source::Spoken
    }
}

/// The digits of a code you read out, in order.
///
/// Speech recognition gives "four eight two nine one seven", "482 917",
/// "4-8-2-9-1-7" or a mix. "Double five" and "triple one" are how people read
/// codes aloud, so they count. Words that are also ordinary words ("to",
/// "for", "oh") only count once a real digit has been heard, so "type my
/// code for google" doesn't become a 4. Returns 4 to 8 digits or nothing.
pub fn code_in_words(said: &str) -> Option<String> {
    let t = said.to_lowercase().replace(['-', ',', '.'], " ");
    let words: Vec<&str> = t.split_whitespace().collect();
    let mut best = String::new();
    let mut run = String::new();
    let mut repeat = 1usize;
    let finish = |run: &mut String, best: &mut String| {
        if run.len() >= 4 && run.len() <= 8 && run.len() > best.len() {
            *best = run.clone();
        }
        run.clear();
    };
    for (i, w) in words.iter().copied().enumerate() {
        // "Oh" and "o" are how a leading zero is read out. Before any digit
        // they only count when a digit follows, so "oh one seven…" keeps its
        // zero and "oh, the code" doesn't become one.
        let next_is_digit = words.get(i + 1).map_or(false, |n| {
            n.chars().all(|c| c.is_ascii_digit())
                || matches!(*n, "zero" | "one" | "two" | "three" | "four" | "five" | "six" | "seven" | "eight" | "nine" | "double" | "triple")
        });
        if w == "double" {
            repeat = 2;
            continue;
        }
        if w == "triple" {
            repeat = 3;
            continue;
        }
        if w.chars().all(|c| c.is_ascii_digit()) {
            for _ in 0..repeat {
                run.push_str(w);
            }
            repeat = 1;
            continue;
        }
        let word_digit = WORD_DIGITS.iter().find(|(name, _)| *name == w).map(|(_, d)| *d);
        let ambiguous = matches!(w, "o" | "oh" | "to" | "too" | "for" | "fore" | "won" | "ate" | "tree");
        match word_digit {
            Some(d) if !ambiguous || !run.is_empty() || (matches!(w, "o" | "oh") && next_is_digit) => {
                for _ in 0..repeat {
                    run.push(d);
                }
                repeat = 1;
            }
            _ => {
                repeat = 1;
                finish(&mut run, &mut best);
            }
        }
    }
    finish(&mut run, &mut best);
    (!best.is_empty()).then_some(best)
}

/// Words that sit beside a code in the messages sites send.
const NEAR_A_CODE: &[&str] = &[
    "code", "verification", "verify", "one-time", "one time", "otp", "passcode",
    "security code", "2-step", "two-step", "two-factor", "2fa", "sign-in", "sign in",
    "login", "log in", "confirm", "authentication", "pin",
];

/// The code in one message, if it has one.
///
/// A message with no code-ish words in it has no code, however many numbers
/// it has: that's what stops an order confirmation's number being typed into
/// a bank's code box.
pub fn code_in_text(subject: &str, body: &str) -> Option<String> {
    let text = format!("{subject}\n{body}");
    let lower = text.to_lowercase();
    let keyword_at: Vec<usize> = NEAR_A_CODE
        .iter()
        .flat_map(|k| lower.match_indices(k).map(|(i, _)| i).collect::<Vec<_>>())
        .collect();
    if keyword_at.is_empty() {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    // Byte offsets for distance; build candidates over chars and track bytes.
    let mut candidates: Vec<(String, usize)> = Vec::new();
    let mut i = 0;
    let mut byte = 0usize;
    let mut byte_at: Vec<usize> = Vec::with_capacity(chars.len() + 1);
    for c in &chars {
        byte_at.push(byte);
        byte += c.len_utf8();
    }
    byte_at.push(byte);
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        // A digit run, allowing one space or dash between groups ("123 456",
        // "123-456") as sites print them.
        let start = i;
        let mut digits = String::new();
        let mut j = i;
        while j < chars.len() {
            if chars[j].is_ascii_digit() {
                digits.push(chars[j]);
                j += 1;
            } else if (chars[j] == ' ' || chars[j] == '-')
                && j + 1 < chars.len()
                && chars[j + 1].is_ascii_digit()
                && j > start
                && chars[j - 1].is_ascii_digit()
                && digits.len() == 3
            {
                j += 1;
            } else {
                break;
            }
        }
        let before = if start > 0 { chars[start - 1] } else { ' ' };
        let after = chars.get(j).copied().unwrap_or(' ');
        let decimal = matches!(after, '.' | ',') && chars.get(j + 1).map_or(false, |c| c.is_ascii_digit());
        let not_money = !matches!(before, '$' | '£' | '€' | '#') && !matches!(after, '%' | ':' | '/') && !decimal;
        let part_of_something = before.is_ascii_alphabetic() || after.is_ascii_alphabetic() || before == '/';
        let year = digits.len() == 4 && (digits.starts_with("19") || digits.starts_with("20"));
        if (4..=8).contains(&digits.len()) && not_money && !part_of_something {
            candidates.push((digits.clone(), byte_at[start]));
            // A year is only a code when it's right after a code word
            // ("Your code: 2024"); scored below by distance, but a year far
            // from any code word is dropped here.
            if year {
                let near = keyword_at.iter().any(|&k| byte_at[start] >= k && byte_at[start] - k < 25);
                if !near {
                    candidates.pop();
                }
            }
        }
        i = j.max(i + 1);
    }
    // A year is only the code when nothing else could be: "© 2026" sits
    // right after "code" in Google's own footer.
    let has_non_year = candidates
        .iter()
        .any(|(d, _)| !(d.len() == 4 && (d.starts_with("19") || d.starts_with("20"))));
    // Which sentence each byte is in: a code shares a sentence with the words
    // that name it ("G-614207 is your Google verification code."), and the
    // footer's "1600 Amphitheatre" doesn't, however near it sits.
    let sentence_of = |at: usize| -> usize {
        let bytes = text.as_bytes();
        (0..at.min(bytes.len()))
            .filter(|&i| {
                matches!(bytes[i], b'!' | b'?' | b'\n')
                    || (bytes[i] == b'.' && bytes.get(i + 1).map_or(true, |c| c.is_ascii_whitespace()))
            })
            .count()
    };
    candidates
        .into_iter()
        .filter(|(d, _)| !(has_non_year && d.len() == 4 && (d.starts_with("19") || d.starts_with("20"))))
        .map(|(d, at)| {
            let mine = sentence_of(at);
            let dist = keyword_at
                .iter()
                .map(|&k| {
                    let apart = if at >= k { at - k } else { k - at };
                    if sentence_of(k) == mine { apart } else { 1_000 + apart }
                })
                .min()
                .unwrap_or(usize::MAX);
            (d, dist)
        })
        .filter(|(_, dist)| *dist < 1_200)
        .min_by_key(|(_, dist)| *dist)
        .map(|(d, _)| d)
}

/// How long a code is worth looking for. Sites expire them in 5 to 10
/// minutes; one older than this is spent or about to be.
pub const FRESH_FOR_SECS: u64 = 10 * 60;

/// The code to use, from what was found.
///
/// Fresh ones only. One from the site you're signing in to beats a newer
/// one from anywhere else; otherwise the newest wins.
pub fn newest(found: &[Found], site: Option<&str>, now: u64) -> Option<Found> {
    let fresh: Vec<&Found> = found
        .iter()
        .filter(|f| f.at == 0 || now.saturating_sub(f.at) <= FRESH_FOR_SECS)
        .collect();
    let from_site = |f: &&Found| {
        site.map(|s| {
            let key = s.to_lowercase();
            let key = key.split('.').find(|p| p.len() > 2 && *p != "www").unwrap_or(&key).to_string();
            f.from.to_lowercase().contains(&key)
        })
        .unwrap_or(false)
    };
    let pick = |v: Vec<&Found>| v.into_iter().max_by_key(|f| f.at).cloned();
    let site_ones: Vec<&Found> = fresh.iter().copied().filter(from_site).collect();
    if !site_ones.is_empty() {
        return pick(site_ones);
    }
    pick(fresh)
}

/// Codes in the text of the Phone Link window (Windows' link to your phone,
/// where your texts show). Each line, or pair of lines, is checked as its own
/// message; the time isn't on screen reliably, so what's found is stamped
/// `now`, and only the last few lines (the newest texts) are read.
pub fn from_phone_link(window_text: &str, now: u64) -> Vec<Found> {
    let lines: Vec<&str> = window_text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let mut out = Vec::new();
    let tail = lines.len().saturating_sub(40);
    for w in lines[tail..].windows(2) {
        if let Some(code) = code_in_text(w[0], w[1]) {
            if !out.iter().any(|f: &Found| f.code == code) {
                out.push(Found { code, from: w[0].to_string(), at: now });
            }
        }
    }
    out
}

/// Does this page want a code? Read from its visible text.
pub fn asks_for_code(page_text: &str) -> bool {
    let t = page_text.to_lowercase();
    [
        "enter the code", "enter code", "verification code", "security code", "one-time code",
        "2-step verification", "two-step verification", "two-factor", "authentication code",
        "we sent a code", "we've sent a code", "we texted", "enter the 6-digit", "enter your code",
        "check your email for a code", "authenticator app",
    ]
    .iter()
    .any(|p| t.contains(p))
}

/// The page script that puts a code in the code box.
///
/// Returns `filled` (one box), `split` (the six-little-boxes kind), `none`,
/// `many:N` (several boxes that could each be it, so none is guessed) or
/// `password` (the only candidate is a password box, which a code never goes
/// in). Fills by setting the value and firing the events a page's own
/// scripts listen for, so frameworks that ignore `.value` still see it.
pub fn code_box_js(code: &str) -> String {
    let c = crate::cdp::js_str(code);
    format!(
        "(() => {{
          const code = '{c}';
          const seen = e => {{ const r = e.getBoundingClientRect(); const s = getComputedStyle(e);
            return r.width > 0 && r.height > 0 && s.visibility !== 'hidden' && s.display !== 'none' && !e.disabled; }};
          const set = (e, v) => {{
            const proto = Object.getPrototypeOf(e);
            const d = Object.getOwnPropertyDescriptor(proto, 'value');
            if (d && d.set) d.set.call(e, v); else e.value = v;
            e.dispatchEvent(new Event('input', {{bubbles:true}}));
            e.dispatchEvent(new Event('change', {{bubbles:true}}));
          }};
          const inputs = Array.from(document.querySelectorAll('input')).filter(seen)
            .filter(e => !['hidden','checkbox','radio','submit','button','email'].includes((e.type||'').toLowerCase()));
          const singles = inputs.filter(e => e.maxLength === 1);
          if (singles.length >= 4 && singles.length === code.length) {{
            singles.forEach((e, i) => {{ e.focus(); set(e, code[i]); }});
            return 'split';
          }}
          const looks = e => {{
            if ((e.autocomplete||'').toLowerCase() === 'one-time-code') return true;
            const hay = [e.name, e.id, e.placeholder, e.getAttribute('aria-label')].filter(Boolean).join(' ').toLowerCase();
            return /(code|otp|totp|2fa|mfa|verif|token|passcode|pin)/.test(hay);
          }};
          const boxes = inputs.filter(looks);
          const plain = boxes.filter(e => (e.type||'').toLowerCase() !== 'password');
          if (plain.length === 0) return boxes.length ? 'password' : 'none';
          if (plain.length > 1) return 'many:' + plain.length;
          plain[0].focus(); set(plain[0], code);
          return 'filled';
        }})()"
    )
}

/// What came of putting the code in.
#[derive(Debug, Clone, PartialEq)]
pub enum Filled {
    Done,
    NoBox,
    SeveralBoxes(usize),
    OnlyAPasswordBox,
}

pub fn code_box_result(result: &str) -> Filled {
    match result {
        "filled" | "split" => Filled::Done,
        "password" => Filled::OnlyAPasswordBox,
        r if r.starts_with("many:") => Filled::SeveralBoxes(r[5..].parse().unwrap_or(2)),
        _ => Filled::NoBox,
    }
}

/// Say a code back so you can check it against your phone: digit by digit,
/// in pairs, which is how people compare codes.
pub fn read_out(code: &str) -> String {
    code.chars()
        .collect::<Vec<_>>()
        .chunks(3)
        .map(|c| c.iter().map(|d| d.to_string()).collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join(", ")
}

/// What Atlas says when it doesn't have a code to type.
pub fn ask_for_it(site: Option<&str>) -> String {
    let on = site.map(|s| format!(" for {s}")).unwrap_or_default();
    format!(
        "Read me the code{on}, or say \"it's in my email\" or \"it's in my texts\" and I'll find it."
    )
}

/// Nothing fresh was found in email or texts.
pub fn none_found(source: Source, site: Option<&str>) -> String {
    let where_ = match source {
        Source::Email => "in your email from the last ten minutes",
        Source::Texts => "in your texts on Phone Link",
        Source::Spoken => "in what you said",
    };
    let for_site = site.map(|s| format!(" from {s}")).unwrap_or_default();
    format!("I didn't find a code{for_site} {where_}. Read it out and I'll type it.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_become_digits() {
        assert_eq!(code_in_words("the code is four eight two nine one seven").as_deref(), Some("482917"));
        assert_eq!(code_in_words("type 482 917").as_deref(), Some("482917"));
        assert_eq!(code_in_words("type my code for google").as_deref(), None);
        assert_eq!(code_in_words("one two double five").as_deref(), Some("1255"));
    }

    #[test]
    fn the_code_not_the_year() {
        let body = "Your Google verification code is 614 207. © 2026 Google LLC, 1600 Amphitheatre";
        assert_eq!(code_in_text("Security code", body).as_deref(), Some("614207"));
        assert_eq!(code_in_text("Your order", "Order 55512345 total $1299").as_deref(), None);
    }
}
