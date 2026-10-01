//! Turning written text into something a voice can say.
//!
//! Piper reads what it is given. Handed "$2.35" it says "dollar two point
//! three five" or worse; handed "mph" it spells the letters; handed a
//! markdown table it reads the pipes and dashes aloud. None of that is the
//! model being bad — it is being asked to speak text that was written to be
//! *seen*. This is the layer that was missing: everything bound for the
//! speaker passes through here first, so numbers, money, units and symbols
//! come out as words and the things that only make sense on a screen are
//! dropped rather than spelled.
//!
//! It is deliberately small and rule-based. A general text-to-speech
//! normaliser is a research project; this handles the cases that actually
//! turn up in what Atlas says — a price, a percentage, a time, a count, a
//! unit — and leaves ordinary prose untouched. What it cannot say cleanly it
//! leaves alone rather than guessing, because a word read oddly is a smaller
//! failure than a sentence mangled.
//!
//! Written text is for the eye; spoken text is for the ear, once, with no
//! chance to re-read. That is the whole reason this exists, and the rule
//! behind every choice in it.

/// Rewrite a line so a text-to-speech voice says it the way you'd read it
/// aloud. Idempotent on plain prose: text with no numbers, symbols or markup
/// comes back unchanged.
pub fn for_speech(text: &str) -> String {
    let mut s = strip_unspeakable(&without_technical_detail(text));
    s = money(&s);
    s = percentages(&s);
    s = units(&s);
    s = symbols(&s);
    collapse_spaces(&s)
}

/// The technical part of a line, left on the screen and in the log and not
/// read out (30 Sep 2026: "the reply to Sam didn't go: 535 5.7.8 Username
/// and Password not accepted (os error 10054)" was read aloud whole).
///
/// - a bracketed aside that reads as machinery -- an error code, a path, a
///   status, `key=value` -- is dropped;
/// - a web address is said as its site;
/// - a file path is said as its file name;
/// - after the first colon of a "couldn't/didn't/failed" sentence, a reason
///   that is mostly codes and symbols is cut, and "-- the details are on the
///   screen" said instead.
pub fn without_technical_detail(text: &str) -> String {
    // Bracketed machinery.
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '(' {
            if let Some(len) = chars[i + 1..].iter().position(|c| *c == ')') {
                let inside: String = chars[i + 1..i + 1 + len].iter().collect();
                if looks_technical(&inside) {
                    // Drop the space before it too.
                    while out.ends_with(' ') {
                        out.pop();
                    }
                    i += len + 2;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    // Addresses and paths, word by word.
    let words: Vec<String> = out
        .split(' ')
        .map(|w| {
            let core = w.trim_end_matches(['.', ',', ';', ':', ')', '"', '\'']);
            let tail = &w[core.len()..];
            if let Some(rest) = core.strip_prefix("https://").or_else(|| core.strip_prefix("http://")) {
                let host = rest.split('/').next().unwrap_or(rest).trim_start_matches("www.");
                return format!("{host}{tail}");
            }
            let pathy = (core.contains('\\') && core.len() > 3) || (core.starts_with('/') && core[1..].contains('/'));
            if pathy {
                let name = core.rsplit(['\\', '/']).find(|p| !p.is_empty()).unwrap_or(core);
                return format!("{name}{tail}");
            }
            w.to_string()
        })
        .collect();
    let mut out = words.join(" ");
    // A failure's reason that is mostly codes.
    let low = out.to_lowercase();
    let failed = ["couldn't", "could not", "didn't", "did not", "failed", "wasn't able", "can't"].iter().any(|w| low.contains(w));
    if failed {
        if let Some(at) = out.find(": ") {
            let (head, reason) = (&out[..at], &out[at + 2..]);
            let sentence_end = reason.find(". ").map(|e| e + 1).unwrap_or(reason.len());
            let (why, after) = reason.split_at(sentence_end);
            if looks_technical(why) {
                out = format!("{head} -- the details are on the screen.{}", after.trim_start_matches('.'));
            }
        }
    }
    out
}

/// Machinery rather than words: codes, symbols, `key=value`, paths.
fn looks_technical(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return false;
    }
    let low = t.to_lowercase();
    if ["os error", "errno", "exit code", "exit status", "status code", "http/", "0x", "stack", "panicked", "traceback", "hresult"]
        .iter()
        .any(|m| low.contains(m))
    {
        return true;
    }
    if t.contains('=') || t.contains('{') || t.contains("::") || t.contains('\\') || !t.contains(' ') && t.matches('/').count() >= 2 {
        return true;
    }
    let words: Vec<&str> = t.split_whitespace().collect();
    let codey = words
        .iter()
        .filter(|w| {
            let w = w.trim_matches(|c: char| !c.is_alphanumeric());
            // Letters and digits mixed ("E0599", "10054ms"), dotted codes
            // ("5.7.8"), or a shouted constant ("ECONNREFUSED"). Plain
            // numbers, money and percentages are words to a listener.
            let mixed = w.chars().any(|c| c.is_ascii_digit()) && w.chars().any(|c| c.is_ascii_alphabetic());
            let dotted = w.matches('.').count() >= 2 && w.chars().all(|c| c.is_ascii_digit() || c == '.');
            let constant = w.len() >= 6 && w.chars().all(|c| c.is_ascii_uppercase() || c == '_');
            !w.is_empty() && (mixed || dotted || constant)
        })
        .count();
    codey * 3 >= words.len().max(1)
}

/// Markup and glyphs that only mean something on a screen. Read aloud they are
/// noise, so they come out rather than being spelled.
fn strip_unspeakable(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let t = line.trim();
        // A markdown table row or a horizontal rule is not a sentence.
        let is_table = t.starts_with('|') && t.ends_with('|');
        let is_rule = t.len() >= 3 && t.chars().all(|c| c == '-' || c == '=' || c == ' ');
        if is_table || is_rule {
            continue;
        }
        for ch in line.chars() {
            match ch {
                // Emphasis and code markers, read as themselves otherwise.
                '*' | '_' | '`' | '#' | '~' => {}
                // Emoji and other symbol-class glyphs: skipped, not named.
                c if is_emoji(c) => {}
                c => out.push(c),
            }
        }
        out.push('\n');
    }
    // A single trailing newline from the loop is not meaningful.
    out.trim_end().to_string()
}

fn is_emoji(c: char) -> bool {
    let n = c as u32;
    (0x1F000..=0x1FAFF).contains(&n)
        || (0x2600..=0x27BF).contains(&n)
        || (0x1F1E6..=0x1F1FF).contains(&n)
        || n == 0xFE0F
}

/// "$2.35" -> "2 dollars and 35 cents", "$5" -> "5 dollars", "$1" -> "1 dollar".
/// Only the leading-symbol form, because that is the one a voice trips on.
fn money(text: &str) -> String {
    rewrite_matches(text, '$', |after| {
        let (num, rest_len) = take_number(after)?;
        let spoken = match num {
            Amount::Whole(d) => format!("{d} {}", plural(d, "dollar", "dollars")),
            Amount::WithCents(d, c) => {
                let dollars = format!("{d} {}", plural(d, "dollar", "dollars"));
                if c == 0 {
                    dollars
                } else {
                    format!("{dollars} and {c} {}", plural(c as u64, "cent", "cents"))
                }
            }
        };
        Some((spoken, rest_len))
    })
}

/// "35%" -> "35 percent". The symbol only, so "percent" written out is left
/// as it is.
fn percentages(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '%' && !out.is_empty() && out.ends_with(|c: char| c.is_ascii_digit()) {
            out.push_str(" percent");
            i += 1;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Common units that a voice spells rather than says, when they follow a
/// number: "5mph" / "5 mph" -> "5 miles per hour". Kept to the handful Atlas
/// actually produces; an unknown unit is left exactly as written.
fn units(text: &str) -> String {
    // Longest first, so "kb" is not eaten before "kbps".
    const UNITS: &[(&str, &str, &str)] = &[
        ("mph", "mile per hour", "miles per hour"),
        ("km/h", "kilometre per hour", "kilometres per hour"),
        ("kbps", "kilobit per second", "kilobits per second"),
        ("mbps", "megabit per second", "megabits per second"),
        ("gb", "gigabyte", "gigabytes"),
        ("mb", "megabyte", "megabytes"),
        ("kb", "kilobyte", "kilobytes"),
        ("kg", "kilogram", "kilograms"),
        ("km", "kilometre", "kilometres"),
        ("cm", "centimetre", "centimetres"),
        ("mm", "millimetre", "millimetres"),
        ("ml", "millilitre", "millilitres"),
        ("hr", "hour", "hours"),
        ("hrs", "hour", "hours"),
        ("min", "minute", "minutes"),
        ("mins", "minute", "minutes"),
    ];
    let mut result = text.to_string();
    for (abbr, one, many) in UNITS {
        result = rewrite_unit(&result, abbr, one, many);
    }
    result
}

/// A number immediately before `abbr` (optionally one space) becomes the
/// number plus the spoken unit, pluralised by the number. The unit must be a
/// standalone token — bounded on the right by a non-letter — so "min" in
/// "minimum" is never touched.
fn rewrite_unit(text: &str, abbr: &str, one: &str, many: &str) -> String {
    let lower = text.to_lowercase();
    let mut out = String::with_capacity(text.len());
    let bytes: Vec<char> = text.chars().collect();
    let lbytes: Vec<char> = lower.chars().collect();
    let alen = abbr.chars().count();
    let mut i = 0;
    while i < bytes.len() {
        // Try to match: digits, optional single space, then abbr, then a
        // non-letter boundary.
        if bytes[i].is_ascii_digit() {
            // consume the number
            let num_start = i;
            let mut j = i;
            while j < bytes.len()
                && (bytes[j].is_ascii_digit() || bytes[j] == '.' || bytes[j] == ',')
            {
                j += 1;
            }
            let mut k = j;
            let had_space = k < bytes.len() && bytes[k] == ' ';
            if had_space {
                k += 1;
            }
            let matches_abbr = k + alen <= lbytes.len()
                && lbytes[k..k + alen].iter().collect::<String>() == *abbr
                && (k + alen == bytes.len() || !bytes[k + alen].is_alphabetic());
            if matches_abbr {
                let num: String = bytes[num_start..j].iter().collect();
                let n = num
                    .replace(',', "")
                    .parse::<f64>()
                    .ok()
                    .map(|f| (f - 1.0).abs() < f64::EPSILON)
                    .unwrap_or(false);
                out.push_str(&num);
                out.push(' ');
                out.push_str(if n { one } else { many });
                i = k + alen;
                continue;
            }
            // Not a unit; emit the number as-is and carry on.
            out.extend(&bytes[num_start..j]);
            i = j;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// The handful of symbols that read wrong. "&" between words is "and";
/// "x" between numbers ("3 x 4") is "by"; "/" between words is "or".
fn symbols(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '&' {
            out.push_str("and");
            i += 1;
            continue;
        }
        // "number x number" -> "number by number", only with spaces so a word
        // like "box" is safe and "3x4" is left for the eye.
        if (c == 'x' || c == '×')
            && between_numbers(&chars, i)
        {
            out.push_str("by");
            i += 1;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

fn between_numbers(chars: &[char], i: usize) -> bool {
    let left = i >= 2 && chars[i - 1] == ' ' && chars[i - 2].is_ascii_digit();
    let right = i + 2 < chars.len() && chars[i + 1] == ' ' && chars[i + 2].is_ascii_digit();
    left && right
}

fn collapse_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last_space = false;
    for c in text.chars() {
        if c == ' ' {
            if !last_space {
                out.push(' ');
            }
            last_space = true;
        } else {
            out.push(c);
            last_space = false;
        }
    }
    out.trim().to_string()
}

enum Amount {
    Whole(u64),
    WithCents(u64, u8),
}

/// Parse a number at the start of `s` for the money rule. Returns the amount
/// and how many characters it spanned.
fn take_number(s: &str) -> Option<(Amount, usize)> {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut digits = String::new();
    while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == ',') {
        if chars[i].is_ascii_digit() {
            digits.push(chars[i]);
        }
        i += 1;
    }
    if digits.is_empty() {
        return None;
    }
    let dollars: u64 = digits.parse().ok()?;
    // Optional ".cc"
    if i < chars.len() && chars[i] == '.' {
        let mut cents = String::new();
        let mut j = i + 1;
        while j < chars.len() && chars[j].is_ascii_digit() && cents.len() < 2 {
            cents.push(chars[j]);
            j += 1;
        }
        if !cents.is_empty() {
            while cents.len() < 2 {
                cents.push('0');
            }
            let c: u8 = cents.parse().ok()?;
            return Some((Amount::WithCents(dollars, c), j));
        }
    }
    Some((Amount::Whole(dollars), i))
}

fn plural(n: u64, one: &str, many: &str) -> String {
    if n == 1 { one.to_string() } else { many.to_string() }
}

/// Walk `text`, and wherever `marker` appears, hand what follows to `f`. If it
/// returns a replacement and how many following characters it consumed, the
/// marker and those characters are replaced; otherwise the marker is kept.
fn rewrite_matches(
    text: &str,
    marker: char,
    f: impl Fn(&str) -> Option<(String, usize)>,
) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == marker {
            let after: String = chars[i + 1..].iter().collect();
            if let Some((spoken, consumed)) = f(&after) {
                out.push_str(&spoken);
                i += 1 + consumed;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_prose_is_untouched() {
        let s = "Let's meet at the cafe and talk it over.";
        assert_eq!(for_speech(s), s);
    }

    #[test]
    fn money_is_spoken_as_words() {
        assert_eq!(for_speech("It's $2.35 today"), "It's 2 dollars and 35 cents today");
        assert_eq!(for_speech("$5 flat"), "5 dollars flat");
        assert_eq!(for_speech("$1 each"), "1 dollar each");
        assert_eq!(for_speech("$1.01"), "1 dollar and 1 cent");
        assert_eq!(for_speech("$1,200"), "1200 dollars");
    }

    #[test]
    fn percentages_become_percent() {
        assert_eq!(for_speech("up 35%"), "up 35 percent");
        // A bare % with no number before it is left alone.
        assert_eq!(for_speech("100% sure"), "100 percent sure");
    }

    #[test]
    fn units_are_read_not_spelled() {
        assert_eq!(for_speech("go 5mph"), "go 5 miles per hour");
        assert_eq!(for_speech("1mph limit"), "1 mile per hour limit");
        assert_eq!(for_speech("40 mins left"), "40 minutes left");
        assert_eq!(for_speech("free 20gb"), "free 20 gigabytes");
    }

    #[test]
    fn a_unit_inside_a_word_is_not_touched() {
        // "min" in "minimum", "km" nowhere near a number.
        assert_eq!(for_speech("the minimum is fine"), "the minimum is fine");
        assert_eq!(for_speech("kilometres ahead"), "kilometres ahead");
    }

    #[test]
    fn symbols_that_read_wrong_are_spoken() {
        assert_eq!(for_speech("cats & dogs"), "cats and dogs");
        assert_eq!(for_speech("a 3 x 4 grid"), "a 3 by 4 grid");
        // "box" must survive — the x is inside a word.
        assert_eq!(for_speech("a box of it"), "a box of it");
    }

    #[test]
    fn screen_only_markup_is_dropped() {
        assert_eq!(for_speech("**bold** and `code`"), "bold and code");
        let table = "Here it is:\n| a | b |\n| 1 | 2 |\ndone";
        assert_eq!(for_speech(table), "Here it is:\ndone");
    }

    #[test]
    fn emoji_are_dropped_not_named() {
        let out = for_speech("nice work 🎉 done");
        assert!(!out.contains('🎉'));
        assert!(out.contains("nice work"));
        assert!(out.contains("done"));
    }

    #[test]
    fn it_is_idempotent() {
        let once = for_speech("$2.35 at 35% over 5mph");
        assert_eq!(for_speech(&once), once, "running it twice must not change it again");
    }
}
