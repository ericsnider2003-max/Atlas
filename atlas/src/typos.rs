//! Forgiving a typo without matching everything.
//!
//! **Source:** Meilisearch's typo-tolerance rules (`meilisearch/meilisearch`,
//! Community Edition, MIT): words shorter than 5 characters must match
//! exactly, 5–8 characters may carry one typo, 9 or more may carry two, and a
//! typo on the *first* character counts as two. Distance is optimal-string-
//! alignment Damerau–Levenshtein (a swap of two neighbours is one typo, the
//! commonest real typing error). Clean-room.
//!
//! **Why Atlas wants it — and why this does not break `palette`'s rule.**
//! `palette::score` is "deliberately not fuzzy": a matcher that returns
//! something for every query means the palette never says "I don't have
//! that". That rule is right, and it is exactly what Meilisearch's thresholds
//! protect: "banana" is never within two typos of "settings", but "setings"
//! and "sttings" are within one. Today those return nothing and the user
//! learns the wrong lesson — that the thing is not there. The proposal is a
//! *fallback*: only when the exact matcher finds nothing, and said as "did you
//! mean Settings?", never silently substituted.

/// Typos a word of this many characters may carry.
pub fn allowance(word_chars: usize) -> usize {
    match word_chars {
        0..=4 => 0,
        5..=8 => 1,
        _ => 2,
    }
}

/// Optimal-string-alignment distance, or `None` once it must exceed `max`
/// (the early exit is what makes checking every palette entry cheap).
pub fn osa(a: &str, b: &str, max: usize) -> Option<usize> {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > max {
        return None;
    }
    let (n, m) = (a.len(), b.len());
    let mut prev2 = vec![0usize; m + 1];
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut cur = vec![0usize; m + 1];
    for i in 1..=n {
        cur[0] = i;
        let mut row_min = cur[0];
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(prev2[j - 2] + 1);
            }
            cur[j] = v;
            row_min = row_min.min(v);
        }
        if row_min > max {
            return None;
        }
        std::mem::swap(&mut prev2, &mut prev);
        std::mem::swap(&mut prev, &mut cur);
    }
    (prev[m] <= max).then_some(prev[m])
}

/// Typos between a typed word and a candidate word, Meilisearch-counted
/// (first-letter typo = 2), or `None` if over the allowance for the typed
/// word's length.
fn typos(typed: &str, word: &str) -> Option<usize> {
    let t = typed.to_lowercase();
    let w = word.to_lowercase();
    let budget = allowance(t.chars().count());
    let d = osa(&t, &w, budget)?;
    let first_differs = t.chars().next() != w.chars().next();
    let counted = d + usize::from(first_differs && d > 0);
    (counted <= budget).then_some(counted)
}

/// As `typos`, but the candidate may be longer: the typed word is matched
/// against the candidate's prefixes (the last word of a query is usually
/// unfinished — Meilisearch treats it the same way).
fn prefix_typos(typed: &str, word: &str) -> Option<usize> {
    let wc: Vec<char> = word.to_lowercase().chars().collect();
    let tl = typed.chars().count();
    let budget = allowance(tl);
    let lo = tl.saturating_sub(budget).max(1);
    let hi = (tl + budget).min(wc.len());
    (lo..=hi)
        .filter_map(|k| typos(typed, &wc[..k].iter().collect::<String>()))
        .min()
}

/// Best candidates for a multi-word query: every typed word must land on some
/// word of the candidate within its allowance. Returns `(index, total typos)`,
/// fewest typos first. Zero-typo candidates, if any, are returned alone — a
/// typo match is only ever a fallback.
pub fn suggest(query: &str, candidates: &[&str]) -> Vec<(usize, usize)> {
    let q: Vec<&str> = query.split_whitespace().collect();
    if q.is_empty() {
        return vec![];
    }
    let mut out: Vec<(usize, usize)> = candidates
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let words: Vec<&str> = c.split_whitespace().collect();
            let mut total = 0;
            for (qi, qw) in q.iter().enumerate() {
                let last = qi == q.len() - 1;
                let best = words
                    .iter()
                    .filter_map(|w| if last { prefix_typos(qw, w) } else { typos(qw, w) })
                    .min()?;
                total += best;
            }
            Some((i, total))
        })
        .collect();
    out.sort_by_key(|(i, t)| (*t, *i));
    if out.first().map(|x| x.1) == Some(0) {
        out.retain(|x| x.1 == 0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds_match_meilisearch() {
        assert_eq!(allowance(4), 0);
        assert_eq!(allowance(5), 1);
        assert_eq!(allowance(8), 1);
        assert_eq!(allowance(9), 2);
    }

    #[test]
    fn transposition_is_one_typo() {
        assert_eq!(osa("recieve", "receive", 2), Some(1));
        assert_eq!(osa("abc", "abc", 0), Some(0));
        assert_eq!(osa("kitten", "sitting", 2), None);
        assert_eq!(osa("kitten", "sitting", 3), Some(3));
    }

    #[test]
    fn first_letter_typo_costs_two() {
        assert_eq!(typos("setings", "settings"), Some(1));
        assert_eq!(typos("wettings", "settings"), None); // 8 chars, 1 allowed, first-letter = 2
        assert_eq!(typos("calendar", "calender"), Some(1));
        assert_eq!(typos("mail", "mial"), None); // 4 chars: exact only
    }

    #[test]
    fn the_palette_rule_holds() {
        let entries = ["Settings", "Open calendar", "Track a new account", "Mail"];
        assert_eq!(suggest("setings", &entries), vec![(0, 1)]);
        assert_eq!(suggest("calender", &entries), vec![(1, 1)]);
        assert!(suggest("banana", &entries).is_empty());
        assert!(suggest("xyz", &entries).is_empty());
        // prefix on the last word: "open cale" -> calendar
        assert_eq!(suggest("open cale", &entries), vec![(1, 0)]);
    }

    #[test]
    fn exact_beats_and_hides_typo_matches() {
        let entries = ["mail", "mall"];
        assert_eq!(suggest("mail", &entries), vec![(0, 0)]);
    }
}
