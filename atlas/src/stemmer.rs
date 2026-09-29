//! "trading", "trades" and "traded" are one word to a person. Porter2 makes
//! them one word to recall.
//!
//! **Source:** the Snowball English (Porter2) stemmer, Martin Porter —
//! `snowballstem/snowball`, BSD-3-Clause; `CurrySoftware/rust-stemmers` (MIT)
//! read as a reference. Clean-room from the published algorithm description
//! (snowballstem.org/algorithms/english/stemmer.html).
//!
//! **Why Atlas wants it.** `recall::word_score` matches `*w == q` — exact
//! words. A note that says "the trades closed early" is invisible to "what
//! did I trade". The meaning encoder papers over it when installed; this makes
//! the words path right on its own, which matters because the words path is
//! the one that always runs (no model, no download).
//!
//! Stemming is for *matching*, never for display: a stem like "happili" is
//! not a word and is never shown to anyone.

fn is_vowel(c: u8) -> bool {
    matches!(c, b'a' | b'e' | b'i' | b'o' | b'u' | b'y')
}

fn is_double(w: &[u8]) -> bool {
    let n = w.len();
    n >= 2 && w[n - 1] == w[n - 2] && matches!(w[n - 1], b'b' | b'd' | b'f' | b'g' | b'm' | b'n' | b'p' | b'r' | b't')
}

fn is_li_ending(c: u8) -> bool {
    matches!(c, b'c' | b'd' | b'e' | b'g' | b'h' | b'k' | b'm' | b'n' | b'r' | b't')
}

/// Region after the first non-vowel following a vowel, starting at `from`.
fn region_after(w: &[u8], from: usize) -> usize {
    let mut i = from;
    while i + 1 < w.len() {
        if is_vowel(w[i]) && !is_vowel(w[i + 1]) {
            return i + 2;
        }
        i += 1;
    }
    w.len()
}

fn ends_short_syllable(w: &[u8]) -> bool {
    let n = w.len();
    if n == 2 {
        return is_vowel(w[0]) && !is_vowel(w[1]);
    }
    n >= 3
        && !is_vowel(w[n - 3])
        && is_vowel(w[n - 2])
        && !is_vowel(w[n - 1])
        && !matches!(w[n - 1], b'w' | b'x' | b'Y')
}

fn contains_vowel(w: &[u8]) -> bool {
    w.iter().any(|c| is_vowel(*c))
}

/// The longest suffix from `list` that `w` ends with.
fn longest<'a>(w: &[u8], list: &[&'a str]) -> Option<&'a str> {
    list.iter().filter(|s| w.ends_with(s.as_bytes())).max_by_key(|s| s.len()).copied()
}

fn replace(w: &mut Vec<u8>, suffix_len: usize, with: &str) {
    w.truncate(w.len() - suffix_len);
    w.extend_from_slice(with.as_bytes());
}

/// Stem one lower-case word. Non-ASCII words are returned unchanged.
pub fn stem(word: &str) -> String {
    let lower = word.to_lowercase();
    if !lower.is_ascii() || lower.len() <= 2 {
        return lower;
    }
    match lower.as_str() {
        "skis" => return "ski".into(),
        "skies" => return "sky".into(),
        "dying" => return "die".into(),
        "lying" => return "lie".into(),
        "tying" => return "tie".into(),
        "idly" => return "idl".into(),
        "gently" => return "gentl".into(),
        "ugly" => return "ugli".into(),
        "early" => return "earli".into(),
        "only" => return "onli".into(),
        "singly" => return "singl".into(),
        "sky" | "news" | "howe" | "atlas" | "cosmos" | "bias" | "andes" => return lower,
        _ => {}
    }
    let mut w: Vec<u8> = lower.trim_start_matches('\'').bytes().collect();
    if w.len() <= 2 {
        return String::from_utf8(w).unwrap_or_default();
    }
    // Consonant y → Y
    if w[0] == b'y' {
        w[0] = b'Y';
    }
    for i in 1..w.len() {
        if w[i] == b'y' && is_vowel(w[i - 1]) {
            w[i] = b'Y';
        }
    }
    let r1 = if w.starts_with(b"gener") || w.starts_with(b"arsen") {
        5
    } else if w.starts_with(b"commun") {
        6
    } else {
        region_after(&w, 0)
    };
    let r2 = region_after(&w, r1);

    // Step 0
    if let Some(s) = longest(&w, &["'s'", "'s", "'"]) {
        let n = s.len();
        w.truncate(w.len() - n);
    }

    // Step 1a
    if let Some(s) = longest(&w, &["sses", "ied", "ies", "us", "ss", "s"]) {
        match s {
            "sses" => replace(&mut w, 4, "ss"),
            "ied" | "ies" => {
                if w.len() > 4 {
                    replace(&mut w, 3, "i")
                } else {
                    replace(&mut w, 3, "ie")
                }
            }
            "s" => {
                // delete if the part before the s (not counting the letter
                // right before it) contains a vowel
                let n = w.len();
                if n >= 3 && contains_vowel(&w[..n - 2]) {
                    w.pop();
                }
            }
            _ => {}
        }
    }

    let as_str = String::from_utf8_lossy(&w).to_string();
    if matches!(
        as_str.as_str(),
        "inning" | "outing" | "canning" | "herring" | "earring" | "proceed" | "exceed" | "succeed"
    ) {
        return as_str;
    }

    // Step 1b
    if let Some(s) = longest(&w, &["eedly", "eed", "ed", "edly", "ing", "ingly"]) {
        let n = s.len();
        match s {
            "eed" | "eedly" => {
                if w.len() - n >= r1 {
                    replace(&mut w, n, "ee");
                }
            }
            _ => {
                if contains_vowel(&w[..w.len() - n]) {
                    w.truncate(w.len() - n);
                    if w.ends_with(b"at") || w.ends_with(b"bl") || w.ends_with(b"iz") {
                        w.push(b'e');
                    } else if is_double(&w) {
                        w.pop();
                    } else if ends_short_syllable(&w) && r1 >= w.len() {
                        w.push(b'e');
                    }
                }
            }
        }
    }

    // Step 1c
    let n = w.len();
    if n > 2 && (w[n - 1] == b'y' || w[n - 1] == b'Y') && !is_vowel(w[n - 2]) {
        w[n - 1] = b'i';
    }

    // Step 2
    const S2: [(&str, &str); 24] = [
        ("tional", "tion"),
        ("enci", "ence"),
        ("anci", "ance"),
        ("abli", "able"),
        ("entli", "ent"),
        ("izer", "ize"),
        ("ization", "ize"),
        ("ational", "ate"),
        ("ation", "ate"),
        ("ator", "ate"),
        ("alism", "al"),
        ("aliti", "al"),
        ("alli", "al"),
        ("fulness", "ful"),
        ("ousli", "ous"),
        ("ousness", "ous"),
        ("iveness", "ive"),
        ("iviti", "ive"),
        ("biliti", "ble"),
        ("bli", "ble"),
        ("ogi", "og"),
        ("fulli", "ful"),
        ("lessli", "less"),
        ("li", ""),
    ];
    let keys: Vec<&str> = S2.iter().map(|(k, _)| *k).collect();
    if let Some(s) = longest(&w, &keys) {
        let n = s.len();
        if w.len() - n >= r1 {
            let to = S2.iter().find(|(k, _)| *k == s).map(|(_, v)| *v).unwrap_or("");
            match s {
                "ogi" => {
                    if w.len() >= 4 && w[w.len() - 4] == b'l' {
                        replace(&mut w, n, to)
                    }
                }
                "li" => {
                    if w.len() >= 3 && is_li_ending(w[w.len() - 3]) {
                        w.truncate(w.len() - 2)
                    }
                }
                _ => replace(&mut w, n, to),
            }
        }
    }

    // Step 3
    const S3: [(&str, &str); 9] = [
        ("tional", "tion"),
        ("ational", "ate"),
        ("alize", "al"),
        ("icate", "ic"),
        ("iciti", "ic"),
        ("ical", "ic"),
        ("ful", ""),
        ("ness", ""),
        ("ative", ""),
    ];
    let keys: Vec<&str> = S3.iter().map(|(k, _)| *k).collect();
    if let Some(s) = longest(&w, &keys) {
        let n = s.len();
        if w.len() - n >= r1 {
            let to = S3.iter().find(|(k, _)| *k == s).map(|(_, v)| *v).unwrap_or("");
            if s == "ative" {
                if w.len() - n >= r2 {
                    w.truncate(w.len() - n);
                }
            } else {
                replace(&mut w, n, to);
            }
        }
    }

    // Step 4
    let s4 = [
        "al", "ance", "ence", "er", "ic", "able", "ible", "ant", "ement", "ment", "ent", "ism", "ate", "iti", "ous",
        "ive", "ize", "ion",
    ];
    if let Some(s) = longest(&w, &s4) {
        let n = s.len();
        if w.len() - n >= r2 {
            if s == "ion" {
                let k = w.len() - n;
                if k >= 1 && (w[k - 1] == b's' || w[k - 1] == b't') {
                    w.truncate(k);
                }
            } else {
                w.truncate(w.len() - n);
            }
        }
    }

    // Step 5
    let n = w.len();
    if n >= 1 && w[n - 1] == b'e' {
        if n > r2 || (n > r1 && !ends_short_syllable(&w[..n - 1])) {
            w.pop();
        }
    } else if n >= 2 && w[n - 1] == b'l' && n > r2 && w[n - 2] == b'l' {
        w.pop();
    }

    for c in w.iter_mut() {
        if *c == b'Y' {
            *c = b'y';
        }
    }
    String::from_utf8(w).unwrap_or_default()
}

/// Lower-case, split on anything that is not a letter or digit, stem each.
/// The drop-in for `recall::words_of` on the matching side.
pub fn stems_of(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| !w.is_empty())
        .map(stem)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_vocabulary_samples() {
        // From the Snowball English voc/output lists.
        for (w, s) in [
            ("caresses", "caress"),
            ("ponies", "poni"),
            ("ties", "tie"),
            ("cries", "cri"),
            ("gaps", "gap"),
            ("gas", "gas"),
            ("kiwis", "kiwi"),
            ("running", "run"),
            ("hopping", "hop"),
            ("hoping", "hope"),
            ("agreed", "agre"),
            ("feed", "feed"),
            ("generously", "generous"),
            ("generate", "generat"),
            ("consign", "consign"),
            ("consigned", "consign"),
            ("consignment", "consign"),
            ("consistently", "consist"),
            ("consistency", "consist"),
            ("consolidated", "consolid"),
            ("conspirators", "conspir"),
            ("constancy", "constanc"),
            ("consolatory", "consolatori"),
            ("happily", "happili"),
            ("skies", "sky"),
            ("dying", "die"),
            ("news", "news"),
            ("atlas", "atlas"),
            ("communism", "communism"),
            ("exceed", "exceed"),
            ("luxuriating", "luxuri"),
        ] {
            assert_eq!(stem(w), s, "{w}");
        }
    }

    #[test]
    fn trade_family_collapses() {
        let s: Vec<String> = ["trade", "trades", "traded", "trading"].iter().map(|w| stem(w)).collect();
        assert!(s.iter().all(|x| x == "trade"), "{s:?}");
        assert_eq!(stem("invoices"), stem("invoice"));
        assert_eq!(stem("meetings"), stem("meeting"));
    }

    #[test]
    fn short_and_non_ascii_untouched() {
        assert_eq!(stem("is"), "is");
        assert_eq!(stem("café"), "café");
        assert_eq!(stems_of("The trades, closed early!"), vec!["the", "trade", "close", "earli"]);
    }
}
