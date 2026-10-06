//! Numbers, money, times and dates as words, before the speech engine sees
//! them.
//!
//! `pronounce` fixed names and tickers; numbers were left to the engine, and
//! the engine reads "1,250" as "one, two hundred fifty", "$3.50" as "dollar
//! three point five zero", "14:30" as "fourteen colon thirty" and
//! "2026-09-24" digit by digit. The classes and their order follow NVIDIA's
//! NeMo text normalisation (Apache-2.0: money, time, date, ordinal, decimal,
//! cardinal, measure); the grammar is written here, for English, in house.

/// 0..999_999_999_999 as English words.
pub fn cardinal(n: u64) -> String {
    const ONES: [&str; 20] = [
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve",
        "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen",
    ];
    const TENS: [&str; 10] = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"];
    fn below_1000(n: u64) -> String {
        let (h, r) = (n / 100, n % 100);
        let mut parts = Vec::new();
        if h > 0 {
            parts.push(format!("{} hundred", ONES[h as usize]));
        }
        if r > 0 || h == 0 {
            parts.push(if r < 20 {
                ONES[r as usize].to_string()
            } else if r % 10 == 0 {
                TENS[(r / 10) as usize].to_string()
            } else {
                format!("{}-{}", TENS[(r / 10) as usize], ONES[(r % 10) as usize])
            });
        }
        parts.join(" ")
    }
    if n < 1000 {
        return below_1000(n);
    }
    let mut parts = Vec::new();
    for (scale, name) in [(1_000_000_000_000u64, "trillion"), (1_000_000_000, "billion"), (1_000_000, "million"), (1_000, "thousand")] {
        if n >= scale && !(n / scale).is_multiple_of(1000) {
            parts.push(format!("{} {name}", below_1000(n / scale % 1000)));
        }
    }
    if !n.is_multiple_of(1000) {
        parts.push(below_1000(n % 1000));
    }
    parts.join(" ")
}

/// 1 → "first", 22 → "twenty-second".
fn ordinal(n: u64) -> String {
    let c = cardinal(n);
    let (head, last) = match c.rfind([' ', '-']) {
        Some(i) => (&c[..=i], &c[i + 1..]),
        None => ("", c.as_str()),
    };
    let last = match last {
        "one" => "first".to_string(),
        "two" => "second".to_string(),
        "three" => "third".to_string(),
        "five" => "fifth".to_string(),
        "eight" => "eighth".to_string(),
        "nine" => "ninth".to_string(),
        "twelve" => "twelfth".to_string(),
        w if w.ends_with('y') => format!("{}ieth", &w[..w.len() - 1]),
        w => format!("{w}th"),
    };
    format!("{head}{last}")
}

/// A year the way it is said: 2026 → "twenty twenty-six", 2005 → "two
/// thousand five", 1990 → "nineteen ninety".
pub fn year(y: u64) -> String {
    if (2000..2010).contains(&y) {
        return cardinal(y);
    }
    let (hi, lo) = (y / 100, y % 100);
    match lo {
        0 => format!("{} hundred", cardinal(hi)),
        1..=9 => format!("{} oh {}", cardinal(hi), cardinal(lo)),
        _ => format!("{} {}", cardinal(hi), cardinal(lo)),
    }
}

/// "1,250.75" → "one thousand two hundred fifty point seven five". Digits
/// after the point are read one by one, the way people read prices and
/// R-multiples.
fn decimal(s: &str) -> Option<String> {
    let (neg, s) = match s.strip_prefix(['-', '−']) {
        Some(r) => (true, r),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let (int, frac) = match s.split_once('.') {
        Some((a, b)) => (a, Some(b)),
        None => (s, None),
    };
    let digits: String = int.chars().filter(|c| *c != ',').collect();
    if digits.is_empty() && frac.is_none() {
        return None;
    }
    if !digits.chars().all(|c| c.is_ascii_digit()) || digits.len() > 15 {
        return None;
    }
    // Commas only in thousands places.
    if int.contains(',') && !int.split(',').skip(1).all(|g| g.len() == 3) {
        return None;
    }
    let mut out = if digits.is_empty() { "zero".to_string() } else { cardinal(digits.parse().ok()?) };
    if let Some(f) = frac {
        if f.is_empty() || !f.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        out.push_str(" point");
        for d in f.chars() {
            out.push(' ');
            out.push_str(&cardinal(d.to_digit(10)? as u64));
        }
    }
    Some(if neg { format!("minus {out}") } else { out })
}

const MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December",
];

fn money(tok: &str) -> Option<String> {
    let (unit, cents, rest) = match tok.chars().next()? {
        '$' => (("dollar", "dollars"), ("cent", "cents"), &tok[1..]),
        '€' => (("euro", "euros"), ("cent", "cents"), &tok['€'.len_utf8()..]),
        '£' => (("pound", "pounds"), ("penny", "pence"), &tok['£'.len_utf8()..]),
        _ => return None,
    };
    let (num, scale) = match rest.chars().last()? {
        'k' | 'K' => (&rest[..rest.len() - 1], Some("thousand")),
        'M' => (&rest[..rest.len() - 1], Some("million")),
        'B' => (&rest[..rest.len() - 1], Some("billion")),
        _ => (rest, None),
    };
    if let Some(scale) = scale {
        return Some(format!("{} {scale} {}", decimal(num)?, unit.1));
    }
    let (whole, part) = match num.split_once('.') {
        Some((w, p)) if p.len() == 2 => (w, Some(p)),
        Some(_) => return Some(format!("{} {}", decimal(num)?, unit.1)),
        None => (num, None),
    };
    let w: u64 = whole.replace(',', "").parse().ok()?;
    let mut out = format!("{} {}", cardinal(w), if w == 1 { unit.0 } else { unit.1 });
    if let Some(p) = part {
        let c: u64 = p.parse().ok()?;
        if c > 0 {
            out = format!("{out} and {} {}", cardinal(c), if c == 1 { cents.0 } else { cents.1 });
        }
    }
    Some(out)
}

/// "14:30" → "fourteen thirty"; "9:05" → "nine oh five"; "7:00" → "seven
/// o'clock". An am/pm right after is kept as "a m" / "p m".
fn time(tok: &str) -> Option<String> {
    let lower = tok.to_lowercase();
    let (t, suffix) = if let Some(t) = lower.strip_suffix("am") {
        (t.to_string(), " a m")
    } else if let Some(t) = lower.strip_suffix("pm") {
        (t.to_string(), " p m")
    } else {
        (lower.clone(), "")
    };
    let (h, m) = t.split_once(':')?;
    if h.is_empty() || h.len() > 2 || m.len() != 2 || !h.chars().chain(m.chars()).all(|c| c.is_ascii_digit()) {
        return None;
    }
    let (h, m): (u64, u64) = (h.parse().ok()?, m.parse().ok()?);
    if h > 23 || m > 59 {
        return None;
    }
    let mins = match m {
        0 if suffix.is_empty() && h > 12 => " hundred".to_string(),
        0 => " o'clock".to_string(),
        1..=9 => format!(" oh {}", cardinal(m)),
        _ => format!(" {}", cardinal(m)),
    };
    Some(format!("{}{mins}{suffix}", cardinal(h)))
}

fn date(tok: &str) -> Option<String> {
    let p: Vec<&str> = tok.split('-').collect();
    if p.len() != 3 || p[0].len() != 4 || p[1].len() != 2 || p[2].len() != 2 {
        return None;
    }
    let (y, mo, d): (u64, usize, u64) = (p[0].parse().ok()?, p[1].parse().ok()?, p[2].parse().ok()?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    Some(format!("{} {}, {}", MONTHS[mo - 1], ordinal(d), year(y)))
}

fn ordinal_token(tok: &str) -> Option<String> {
    let lower = tok.to_lowercase();
    for s in ["st", "nd", "rd", "th"] {
        if let Some(n) = lower.strip_suffix(s) {
            if !n.is_empty() && n.len() <= 6 && n.chars().all(|c| c.is_ascii_digit()) {
                return Some(ordinal(n.parse().ok()?));
            }
        }
    }
    None
}

/// One whitespace-separated token, if it is a number of some kind.
fn token(core: &str, prev: Option<&str>) -> Option<String> {
    if let Some(m) = money(core) {
        return Some(m);
    }
    if let Some(t) = time(core) {
        return Some(t);
    }
    if let Some(d) = date(core) {
        return Some(d);
    }
    if let Some(o) = ordinal_token(core) {
        return Some(o);
    }
    // A range: 10–12, 3-5.
    for dash in ['–', '-'] {
        if let Some((a, b)) = core.split_once(dash) {
            if !a.is_empty() && !b.is_empty() && a.chars().all(|c| c.is_ascii_digit()) && b.chars().all(|c| c.is_ascii_digit()) {
                return Some(format!("{} to {}", decimal(a)?, decimal(b)?));
            }
        }
    }
    // 4k, 2.5M: a number with a magnitude.
    for (suffix, said) in [("k", "thousand"), ("K", "thousand"), ("M", "million"), ("B", "billion")] {
        if let Some(n) = core.strip_suffix(suffix) {
            if n.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                if let Some(d) = decimal(n) {
                    return Some(format!("{d} {said}"));
                }
            }
        }
    }
    // A percentage, an R-multiple, a times.
    for (suffix, said) in [("%", " percent"), ("R", " R"), ("x", " times")] {
        if let Some(n) = core.strip_suffix(suffix) {
            if n.chars().next().map(|c| c.is_ascii_digit() || c == '-' || c == '+' || c == '.').unwrap_or(false) {
                if let Some(d) = decimal(n) {
                    let sign = if n.starts_with('+') { "plus " } else { "" };
                    return Some(format!("{sign}{d}{said}"));
                }
            }
        }
    }
    // A year after a word that introduces one.
    if core.len() == 4 && core.chars().all(|c| c.is_ascii_digit()) {
        let y: u64 = core.parse().ok()?;
        let yearish = prev
            .map(|p| {
                let p = p.to_lowercase();
                ["in", "since", "by", "of", "from", "until", "before", "after"].contains(&p.as_str())
                    || MONTHS.iter().any(|m| m.to_lowercase() == p.trim_end_matches(','))
            })
            .unwrap_or(false);
        if yearish && (1900..2100).contains(&y) {
            return Some(year(y));
        }
    }
    if core.chars().next().map(|c| c.is_ascii_digit() || ((c == '-' || c == '+') && core.len() > 1)).unwrap_or(false) {
        let sign = if core.starts_with('+') { "plus " } else { "" };
        return decimal(core).map(|d| format!("{sign}{d}"));
    }
    None
}

/// Every number-like token in `text` said as words; everything else
/// untouched. Trailing punctuation stays where it was.
pub fn words(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut prev: Option<String> = None;
    let toks: Vec<&str> = text.split_whitespace().collect();
    let mut i = 0;
    while i < toks.len() {
        let w = toks[i];
        if w.starts_with("http://") || w.starts_with("https://") {
            out.push(w.to_string());
            prev = Some(w.to_string());
            i += 1;
            continue;
        }
        // Split off trailing punctuation (but not a % or R that belongs).
        let end = w.trim_end_matches([',', '.', ';', ':', '!', '?', ')']).len();
        let (core, tail) = w.split_at(end);
        let core = core.trim_start_matches(['(', '~']);
        let lead = &w[..w.len() - tail.len() - core.len()];
        // "7:30 pm" as two tokens.
        let mut core_owned = core.to_string();
        let mut used_next = false;
        if let Some(next) = toks.get(i + 1) {
            let n = next.trim_end_matches([',', '.', ';', '!', '?']).to_lowercase();
            if (n == "am" || n == "pm" || n == "a.m" || n == "p.m") && time(core).is_some() {
                core_owned = format!("{core}{}", n.replace('.', ""));
                used_next = true;
            }
        }
        match token(&core_owned, prev.as_deref()) {
            Some(said) => {
                let tail = if used_next {
                    let next = toks[i + 1];
                    &next[next.trim_end_matches([',', '.', ';', '!', '?']).len()..]
                } else {
                    tail
                };
                out.push(format!("{lead}{said}{tail}"));
            }
            None => out.push(w.to_string()),
        }
        prev = Some(core.to_string());
        i += if used_next { 2 } else { 1 };
    }
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_classes() {
        assert_eq!(cardinal(1_250), "one thousand two hundred fifty");
        assert_eq!(cardinal(40), "forty");
        assert_eq!(cardinal(1_000_021), "one million twenty-one");
        assert_eq!(ordinal(22), "twenty-second");
        assert_eq!(ordinal(30), "thirtieth");
        assert_eq!(year(2026), "twenty twenty-six");
        assert_eq!(year(2005), "two thousand five");
        assert_eq!(year(1907), "nineteen oh seven");
    }

    #[test]
    fn in_sentences() {
        assert_eq!(words("It costs $1,250."), "It costs one thousand two hundred fifty dollars.");
        assert_eq!(words("Only $3.50 left"), "Only three dollars and fifty cents left");
        assert_eq!(words("Call at 14:30, or 7:30 pm."), "Call at fourteen thirty, or seven thirty p m.");
        assert_eq!(words("due 2026-09-24"), "due September twenty-fourth, twenty twenty-six");
        assert_eq!(words("up +0.6R, 98% done, 3x faster"), "up plus zero point six R, ninety-eight percent done, three times faster");
        assert_eq!(words("the 3rd of 10–12 trades in 2026"), "the third of ten to twelve trades in twenty twenty-six");
        assert_eq!(words("a budget of €4k"), "a budget of four thousand euros");
        assert_eq!(words("-0.090 R"), "minus zero point zero nine zero R");
        assert_eq!(words("~4k trades"), "~four thousand trades");
    }
}
