//! How many guesses would it take? — a passphrase estimate that counts
//! patterns, not character classes.
//!
//! **Source:** Wheeler (2016), *zxcvbn: Low-Budget Password Strength
//! Estimation* (USENIX Security), and `dropbox/zxcvbn` / the Rust port
//! `shssoichiro/zxcvbn` (both MIT). The frequency lists in
//! `config/guessable/` are the top of theirs (MIT, see LICENSE-zxcvbn.txt).
//! The matchers, guess counts and the minimum-guess search are re-written
//! here, smaller: dictionary (with reversal, capitals and l33t), sequences,
//! repeats, keyboard walks, dates and years, and brute force for the rest;
//! then the cheapest way to cover the whole string, `k! · Π guesses` for k
//! pieces, as the paper does. Score 0–4 at 10³ / 10⁶ / 10⁸ / 10¹⁰ guesses.
//!
//! **Why Atlas wants it.** The vault refused anything under twelve
//! characters and accepted anything over. "password1234" and "aaaaaaaaaaaa"
//! passed; "correct horse battery" is fine and should stay fine. Length was
//! standing in for what actually matters: how early in a guessing list the
//! phrase comes.

const LISTS: [(&str, &str); 6] = [
    ("common passwords", include_str!("../config/guessable/passwords.txt")),
    ("English words", include_str!("../config/guessable/english.txt")),
    ("surnames", include_str!("../config/guessable/surnames.txt")),
    ("first names", include_str!("../config/guessable/male_names.txt")),
    ("first names", include_str!("../config/guessable/female_names.txt")),
    ("film and TV words", include_str!("../config/guessable/tv_and_film.txt")),
];

use std::collections::HashMap;
use std::sync::OnceLock;

fn ranked() -> &'static HashMap<String, (usize, &'static str)> {
    static R: OnceLock<HashMap<String, (usize, &'static str)>> = OnceLock::new();
    R.get_or_init(|| {
        let mut m: HashMap<String, (usize, &'static str)> = HashMap::new();
        for (what, text) in LISTS {
            for (i, w) in text.lines().enumerate() {
                let w = w.trim();
                if w.is_empty() {
                    continue;
                }
                let e = m.entry(w.to_string()).or_insert((i + 1, what));
                if i + 1 < e.0 {
                    *e = (i + 1, what);
                }
            }
        }
        m
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Word { word: String, list: &'static str, reversed: bool, l33t: bool },
    Sequence,
    Repeat,
    Keyboard,
    Date,
    Brute,
}

#[derive(Debug, Clone)]
pub struct Piece {
    pub i: usize,
    pub j: usize, // inclusive, in chars
    pub kind: Kind,
    pub guesses: f64,
}

#[derive(Debug, Clone)]
pub struct Estimate {
    pub guesses_log10: f64,
    pub score: u8,
    pub pieces: Vec<Piece>,
}

impl Estimate {
    /// The weakest thing in it, in words — or None if nothing stands out.
    fn warning(&self) -> Option<String> {
        let chars = self.pieces.last().map(|p| p.j + 1).unwrap_or(0);
        let whole = self.pieces.len() == 1;
        // The piece covering most of it first; failing that, any named piece —
        // "atlas" + "atlas" + "2026" has no single big piece but is still
        // three guessable ones.
        let mut order: Vec<&Piece> = self.pieces.iter().filter(|p| whole || (p.j + 1 - p.i) * 2 >= chars).collect();
        order.extend(self.pieces.iter().filter(|p| !(whole || (p.j + 1 - p.i) * 2 >= chars)));
        for p in order {
            return Some(match &p.kind {
                Kind::Word { word, list, reversed, l33t } => {
                    let how = match (reversed, l33t) {
                        (true, _) => " spelled backwards",
                        (_, true) => " with letters swapped for symbols",
                        _ => "",
                    };
                    if *list == "things about you" {
                        format!("\"{word}\"{how} is one of the first words anyone aiming at this vault would try")
                    } else {
                        format!("\"{word}\"{how} is on the list of {list} guessers try first")
                    }
                }
                Kind::Sequence => "a run like abc or 1234 is one of the first things tried".into(),
                Kind::Repeat => "repeated characters or chunks add almost nothing".into(),
                Kind::Keyboard => "a walk along the keyboard is on every guessing list".into(),
                Kind::Date => "dates and years are guessed early".into(),
                Kind::Brute => continue,
            });
        }
        None
    }

    pub fn say(&self) -> String {
        let label = ["very guessable", "guessable", "somewhat guessable", "hard to guess", "very hard to guess"][self.score as usize];
        let mut s = format!("{label} (about 10^{:.0} guesses)", self.guesses_log10);
        // The weakest piece is only worth naming when it made the whole weak;
        // "the" in a strong sentence is not a warning.
        if let Some(w) = self.warning().filter(|_| self.score <= 2) {
            s.push_str(&format!(" — {w}"));
        }
        s
    }
}

fn l33t_undo(c: char) -> Option<char> {
    Some(match c {
        '4' | '@' => 'a',
        '8' => 'b',
        '(' | '{' | '[' | '<' => 'c',
        '3' => 'e',
        '6' | '9' => 'g',
        '1' | '!' | '|' => 'i',
        '0' => 'o',
        '$' | '5' => 's',
        '+' | '7' => 't',
        '%' => 'x',
        '2' => 'z',
        _ => return None,
    })
}

fn n_ck(n: usize, k: usize) -> f64 {
    if k > n {
        return 0.0;
    }
    let mut r = 1.0;
    for d in 1..=k {
        r = r * (n - k + d) as f64 / d as f64;
    }
    r
}

fn capital_variations(s: &[char]) -> f64 {
    let up = s.iter().filter(|c| c.is_uppercase()).count();
    let low = s.iter().filter(|c| c.is_lowercase()).count();
    if up == 0 {
        return 1.0;
    }
    let first_only = s[0].is_uppercase() && up == 1;
    let last_only = s[s.len() - 1].is_uppercase() && up == 1;
    if first_only || last_only || low == 0 {
        return 2.0;
    }
    (1..=up.min(low)).map(|k| n_ck(up + low, k)).sum::<f64>().max(1.0)
}

fn dictionary(cs: &[char], extra: &[String], out: &mut Vec<Piece>) {
    let n = cs.len();
    let lower: Vec<char> = cs.iter().map(|c| c.to_lowercase().next().unwrap_or(*c)).collect();
    let dict = ranked();
    for i in 0..n {
        for j in i..n.min(i + 30) {
            let slice = &lower[i..=j];
            let plain: String = slice.iter().collect();
            let rev: String = slice.iter().rev().collect();
            let unl33t: Option<String> = if slice.iter().any(|c| l33t_undo(*c).is_some()) {
                Some(slice.iter().map(|c| l33t_undo(*c).unwrap_or(*c)).collect())
            } else {
                None
            };
            let caps = capital_variations(&cs[i..=j]);
            let look = |w: &str| -> Option<(usize, &'static str)> {
                if let Some(k) = extra.iter().position(|e| e == w) {
                    return Some((k + 1, "things about you"));
                }
                dict.get(w).copied()
            };
            if let Some((rank, list)) = look(&plain) {
                out.push(Piece { i, j, kind: Kind::Word { word: plain.clone(), list, reversed: false, l33t: false }, guesses: rank as f64 * caps });
            }
            if j > i && rev != plain {
                if let Some((rank, list)) = look(&rev) {
                    out.push(Piece { i, j, kind: Kind::Word { word: rev.clone(), list, reversed: true, l33t: false }, guesses: rank as f64 * caps * 2.0 });
                }
            }
            if let Some(u) = unl33t {
                if j > i {
                    if let Some((rank, list)) = look(&u) {
                        let subs = slice.iter().filter(|c| l33t_undo(**c).is_some()).count();
                        let l33t = (1..=subs).map(|k| n_ck(slice.len(), k)).sum::<f64>().max(2.0);
                        out.push(Piece { i, j, kind: Kind::Word { word: u, list, reversed: false, l33t: true }, guesses: rank as f64 * caps * l33t });
                    }
                }
            }
        }
    }
}

fn sequences(cs: &[char], out: &mut Vec<Piece>) {
    let n = cs.len();
    let mut i = 0;
    while i + 2 < n {
        let d = cs[i + 1] as i64 - cs[i] as i64;
        if d.abs() != 1 {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j + 1 < n && cs[j + 1] as i64 - cs[j] as i64 == d {
            j += 1;
        }
        if j - i >= 2 {
            let first = cs[i];
            let base = if matches!(first, 'a' | 'A' | 'z' | 'Z' | '0' | '1' | '9') {
                4.0
            } else if first.is_ascii_digit() {
                10.0
            } else {
                26.0
            };
            let dir = if d < 0 { 2.0 } else { 1.0 };
            out.push(Piece { i, j, kind: Kind::Sequence, guesses: base * dir * (j + 1 - i) as f64 });
        }
        i = j;
    }
}

fn repeats(cs: &[char], out: &mut Vec<Piece>) {
    let n = cs.len();
    for i in 0..n {
        for unit in 1..=((n - i) / 2).min(12) {
            let mut j = i + unit;
            while j + unit <= n && cs[j..j + unit] == cs[i..i + unit] {
                j += unit;
            }
            let count = (j - i) / unit;
            if count >= 2 && (unit > 1 || count >= 3) {
                let base = estimate_chars(&cs[i..i + unit], &[]).guesses_log10;
                out.push(Piece { i, j: j - 1, kind: Kind::Repeat, guesses: 10f64.powf(base) * count as f64 });
            }
        }
    }
}

const QWERTY_ROWS: [&str; 4] = ["`1234567890-=", "qwertyuiop[]\\", "asdfghjkl;'", "zxcvbnm,./"];

fn key_at(c: char) -> Option<(i32, i32)> {
    let c = c.to_ascii_lowercase();
    let shifted = "~!@#$%^&*()_+";
    if let Some(x) = shifted.find(c) {
        return Some((0, x as i32));
    }
    QWERTY_ROWS.iter().enumerate().find_map(|(r, row)| row.find(c).map(|x| (r as i32, x as i32 + r as i32 / 2)))
}

fn keyboard(cs: &[char], out: &mut Vec<Piece>) {
    let n = cs.len();
    let adjacent = |a: char, b: char| -> Option<(i32, i32)> {
        let (p, q) = (key_at(a)?, key_at(b)?);
        let d = (q.0 - p.0, q.1 - p.1);
        (d.0.abs() <= 1 && d.1.abs() <= 1 && d != (0, 0)).then_some(d)
    };
    let mut i = 0;
    while i + 2 < n {
        let mut j = i;
        let mut turns = 0;
        let mut last = None;
        while j + 1 < n {
            match adjacent(cs[j], cs[j + 1]) {
                Some(d) => {
                    if last != Some(d) {
                        turns += 1;
                        last = Some(d);
                    }
                    j += 1;
                }
                None => break,
            }
        }
        if j - i >= 2 {
            let (s, d) = (94.0f64, 4.6f64);
            let len = j + 1 - i;
            let mut g = 0.0;
            for l in 2..=len {
                for t in 1..=turns.min(l - 1) {
                    g += n_ck(l - 1, t - 1) * s * d.powi(t as i32);
                }
            }
            out.push(Piece { i, j, kind: Kind::Keyboard, guesses: g * capital_variations(&cs[i..=j]) });
            i = j;
        } else {
            i += 1;
        }
    }
}

fn dates(cs: &[char], out: &mut Vec<Piece>) {
    let n = cs.len();
    let digits = |a: usize, b: usize| cs[a..=b].iter().all(|c| c.is_ascii_digit());
    for i in 0..n {
        // bare years
        if i + 3 < n && digits(i, i + 3) {
            let y: i32 = cs[i..i + 4].iter().collect::<String>().parse().unwrap_or(0);
            if (1900..=2049).contains(&y) {
                out.push(Piece { i, j: i + 3, kind: Kind::Date, guesses: ((y - 2026).abs().max(20)) as f64 });
            }
        }
        // 6–8 digits that read as a date, with or without separators
        for len in [6usize, 8] {
            if i + len <= n && digits(i, i + len - 1) {
                out.push(Piece { i, j: i + len - 1, kind: Kind::Date, guesses: 365.0 * 30.0 });
            }
        }
        for len in [8usize, 10] {
            if i + len <= n {
                let seg: String = cs[i..i + len].iter().collect();
                let parts: Vec<&str> = seg.split(['/', '-', '.', ' ']).collect();
                if parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())) {
                    out.push(Piece { i, j: i + len - 1, kind: Kind::Date, guesses: 365.0 * 30.0 * 4.0 });
                }
            }
        }
    }
}

fn brute(len: usize) -> f64 {
    10f64.powi(len as i32).max(if len == 1 { 11.0 } else { 51.0 })
}

fn estimate_chars(cs: &[char], extra: &[String]) -> Estimate {
    let n = cs.len();
    if n == 0 {
        return Estimate { guesses_log10: 0.0, score: 0, pieces: vec![] };
    }
    let mut ms = Vec::new();
    dictionary(cs, extra, &mut ms);
    sequences(cs, &mut ms);
    if n <= 64 {
        repeats(cs, &mut ms);
    }
    keyboard(cs, &mut ms);
    dates(cs, &mut ms);
    for m in ms.iter_mut() {
        let floor = if m.j == m.i { 11.0 } else { 51.0 };
        m.guesses = m.guesses.max(floor);
    }
    // best[j][k] = min log10(Π guesses) covering 0..=j with k pieces; brute
    // force fills any gap as its own piece.
    let kmax = n.min(20);
    let inf = f64::INFINITY;
    let mut best = vec![vec![(inf, None::<(usize, usize, Option<usize>)>); kmax + 1]; n];
    for j in 0..n {
        for k in 1..=kmax {
            // ends with a brute-force run i..=j
            for i in 0..=j {
                let prev = if i == 0 { if k == 1 { 0.0 } else { inf } } else { best[i - 1][k - 1].0 };
                let v = prev + brute(j + 1 - i).log10();
                if v < best[j][k].0 {
                    best[j][k] = (v, Some((i, k, None)));
                }
            }
            for (mi, m) in ms.iter().enumerate().filter(|(_, m)| m.j == j) {
                let prev = if m.i == 0 { if k == 1 { 0.0 } else { inf } } else { best[m.i - 1][k - 1].0 };
                let v = prev + m.guesses.log10();
                if v < best[j][k].0 {
                    best[j][k] = (v, Some((m.i, k, Some(mi))));
                }
            }
        }
    }
    let log_fact = |k: usize| (1..=k).map(|x| (x as f64).log10()).sum::<f64>();
    let (k, lg) = (1..=kmax)
        .map(|k| (k, best[n - 1][k].0 + log_fact(k)))
        .fold((1, inf), |a, b| if b.1 < a.1 { b } else { a });
    let mut pieces = Vec::new();
    let (mut j, mut kk) = (n - 1, k);
    while let Some((i, _, m)) = best[j][kk].1 {
        pieces.push(match m {
            Some(mi) => ms[mi].clone(),
            None => Piece { i, j, kind: Kind::Brute, guesses: brute(j + 1 - i) },
        });
        if i == 0 {
            break;
        }
        j = i - 1;
        kk -= 1;
    }
    pieces.reverse();
    let score = match lg {
        g if g < 3.0 => 0,
        g if g < 6.0 => 1,
        g if g < 8.0 => 2,
        g if g < 10.0 => 3,
        _ => 4,
    };
    Estimate { guesses_log10: lg, score, pieces }
}

/// Estimate a passphrase. `about_you` are words guessers would try first
/// for this person (their name, "atlas", the business) — lower-cased.
pub fn estimate(phrase: &str, about_you: &[&str]) -> Estimate {
    let cs: Vec<char> = phrase.chars().take(100).collect();
    let extra: Vec<String> = about_you.iter().map(|w| w.to_lowercase()).filter(|w| w.len() > 2).collect();
    let mut e = estimate_chars(&cs, &extra);
    let tail = phrase.chars().count().saturating_sub(100);
    if tail > 0 {
        e.guesses_log10 += tail as f64; // past 100 characters, every one is ~10×
        e.score = 4;
    }
    e
}

/// The rule the vault applies when a passphrase is chosen: refuse the
/// guessable ones (score 0–1, under a million guesses) with the reason, warn
/// on score 2. `Ok(Some(note))` is a warning to say but not a refusal.
pub fn fit_for_the_vault(phrase: &str, about_you: &[&str]) -> Result<Option<String>, String> {
    let e = estimate(phrase, about_you);
    match e.score {
        0 | 1 => Err(format!(
            "that's {} — a guesser gets there early. A few unrelated words in a row is enough",
            e.say()
        )),
        2 => Ok(Some(format!("Accepted, but it's {} — one more unrelated word would put it well out of reach.", e.say()))),
        _ => Ok(None),
    }
}
