//! Is "Jon Smith <jon.smith@acme.com>" the same client as "Smith, John
//! (555) 010-2000"? Record linkage for the client list and contacts.
//!
//! **Sources:** Jaro–Winkler as in `rapidfuzz/strsim-rs` (MIT): prefix scale
//! 0.1, at most 4 prefix characters, and the prefix bonus applied only when
//! the plain Jaro score is above 0.7. The scoring model is Fellegi–Sunter as
//! used by `moj-analytical-services/splink` (MIT): each field compares into a
//! level, each level carries m (P(level | same person)) and u (P(level |
//! different people)), and the evidence adds up as log2(m/u). Clean-room.
//!
//! **Why Atlas wants it.** The business hub wants a client list; mail, vCards
//! (`vformat`), the calendar and the phone all mint contacts, and the same
//! person arrives four ways. `facts.rs` resolves entities by declared alias
//! ("X is also known as Y"); nothing notices an *undeclared* duplicate. This
//! does, and says why — "same email; names 0.96 similar" — so merging stays
//! a yes/no question to the user rather than something Atlas does quietly.
//!
//! The m numbers are **chosen, not measured**: splink estimates them by EM,
//! and with a few hundred contacts that estimate is noise. The u numbers are
//! **measured** when the list is big enough (`Model::measured_on`, 30+
//! contacts): u is how often a level happens between two *different*
//! people, and nearly every pair in a list is two different people, so the
//! level's frequency over all pairs estimates it well — splink's
//! `estimate_u_using_random_sampling`, done exhaustively because the list is
//! small.

use std::collections::{BTreeMap, BTreeSet};

pub fn jaro(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let window = (a.len().max(b.len()) / 2).saturating_sub(1);
    let mut a_hit = vec![false; a.len()];
    let mut b_hit = vec![false; b.len()];
    let mut matches = 0usize;
    for i in 0..a.len() {
        let lo = i.saturating_sub(window);
        let hi = (i + window + 1).min(b.len());
        for j in lo..hi {
            if !b_hit[j] && a[i] == b[j] {
                a_hit[i] = true;
                b_hit[j] = true;
                matches += 1;
                break;
            }
        }
    }
    if matches == 0 {
        return 0.0;
    }
    let (mut t, mut k) = (0usize, 0usize);
    for i in 0..a.len() {
        if a_hit[i] {
            while !b_hit[k] {
                k += 1;
            }
            if a[i] != b[k] {
                t += 1;
            }
            k += 1;
        }
    }
    let m = matches as f64;
    (m / a.len() as f64 + m / b.len() as f64 + (m - t as f64 / 2.0) / m) / 3.0
}

pub fn jaro_winkler(a: &str, b: &str) -> f64 {
    let j = jaro(a, b);
    if j <= 0.7 {
        return j;
    }
    let prefix = a.chars().zip(b.chars()).take(4).take_while(|(x, y)| x == y).count();
    j + prefix as f64 * 0.1 * (1.0 - j)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Contact {
    pub name: String,
    pub email: String,
    pub phone: String,
}

const TITLES: [&str; 8] = ["mr", "mrs", "ms", "miss", "dr", "prof", "sir", "mx"];

/// "Smith, John" → "john smith"; titles and punctuation dropped.
pub fn norm_name(s: &str) -> String {
    let s = match s.split_once(',') {
        Some((last, first)) if !first.trim().is_empty() => format!("{} {}", first.trim(), last.trim()),
        _ => s.to_string(),
    };
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && !TITLES.contains(w))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Lower-case, `+tag` removed; for gmail/googlemail the dots are removed too
/// (Google ignores them; other providers do not, so they are left alone).
pub fn norm_email(s: &str) -> String {
    let s = s.trim().trim_matches(|c| c == '<' || c == '>').to_lowercase();
    let Some((local, domain)) = s.rsplit_once('@') else { return s };
    let local = local.split('+').next().unwrap_or(local);
    let domain = if domain == "googlemail.com" { "gmail.com" } else { domain };
    let local = if domain == "gmail.com" { local.replace('.', "") } else { local.to_string() };
    format!("{local}@{domain}")
}

/// Digits only, last 10 (drops a leading country code 1 on US numbers).
pub fn norm_phone(s: &str) -> String {
    let d: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    if d.len() > 10 {
        d[d.len() - 10..].to_string()
    } else {
        d
    }
}

/// m and u for one comparison level. m chosen; u measured when the list is
/// big enough (`Model::measured_on`).
#[derive(Debug, Clone, Copy)]
pub struct Level {
    pub m: f64,
    pub u: f64,
}
impl Level {
    pub fn weight(&self) -> f64 {
        (self.m / self.u).log2()
    }
}

#[derive(Debug, Clone)]
pub struct Model {
    pub name_exact: Level,
    pub name_close: Level, // JW ≥ 0.92
    pub name_near: Level,  // JW ≥ 0.85
    pub name_else: Level,
    pub email_exact: Level,
    pub email_else: Level,
    pub phone_exact: Level,
    pub phone_else: Level,
    /// Prior odds a random pair is the same person, as log2.
    pub prior: f64,
    pub same_above: f64,
    pub maybe_above: f64,
}

impl Default for Model {
    fn default() -> Self {
        Model {
            name_exact: Level { m: 0.70, u: 0.001 },
            name_close: Level { m: 0.20, u: 0.005 },
            name_near: Level { m: 0.06, u: 0.02 },
            name_else: Level { m: 0.04, u: 0.974 },
            email_exact: Level { m: 0.80, u: 0.0001 },
            email_else: Level { m: 0.20, u: 0.9999 },
            phone_exact: Level { m: 0.75, u: 0.0005 },
            phone_else: Level { m: 0.25, u: 0.9995 },
            // A personal or small-business list: a few hundred people, so a
            // random pair is one person about one time in a hundred.
            prior: (1.0f64 / 100.0).log2(),
            same_above: 0.95,
            maybe_above: 0.5,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    Same,
    Maybe,
    Different,
}

#[derive(Debug, Clone)]
pub struct Match {
    pub probability: f64,
    pub verdict: Verdict,
    /// In words, for the "are these the same person?" question.
    pub because: Vec<String>,
}

/// Which name level a pair falls in: 0 exact, 1 close, 2 near, 3 else;
/// None when either name is missing. The same rule `compare` scores with.
fn name_level(a: &str, b: &str) -> Option<(usize, f64)> {
    let (na, nb) = (norm_name(a), norm_name(b));
    if na.is_empty() || nb.is_empty() {
        return None;
    }
    let sort = |s: &str| {
        let mut v: Vec<&str> = s.split(' ').collect();
        v.sort();
        v.join(" ")
    };
    let jw = jaro_winkler(&na, &nb).max(jaro_winkler(&sort(&na), &sort(&nb)));
    Some(if na == nb || sort(&na) == sort(&nb) {
        (0, 1.0)
    } else if jw >= 0.92 {
        (1, jw)
    } else if jw >= 0.85 {
        (2, jw)
    } else {
        (3, jw)
    })
}

impl Model {
    /// The default model with each u measured on `list`: the share of all
    /// pairs (almost all of them different people) that land in each level.
    /// Too few pairs for a field, and its u stays as chosen. Smoothed by half
    /// a count so a level never seen is rare, not impossible. Returns what
    /// was measured, in words.
    pub fn measured_on(list: &[Contact]) -> (Model, String) {
        let mut m = Model::default();
        if list.len() < 30 {
            return (m, format!("{} contacts is too few to measure from; the weights are the chosen ones", list.len()));
        }
        let (mut name, mut email, mut phone) = ([0f64; 4], [0f64; 2], [0f64; 2]);
        // Every pair up to ~200,000; past that, a fixed stride through them.
        let n = list.len();
        let total = n * (n - 1) / 2;
        let stride = (total / 200_000).max(1);
        let mut k = 0usize;
        for i in 0..n {
            for j in i + 1..n {
                k += 1;
                if k % stride != 0 {
                    continue;
                }
                let (a, b) = (&list[i], &list[j]);
                if let Some((lvl, _)) = name_level(&a.name, &b.name) {
                    name[lvl] += 1.0;
                }
                let (ea, eb) = (norm_email(&a.email), norm_email(&b.email));
                if !ea.is_empty() && !eb.is_empty() {
                    email[if ea == eb { 0 } else { 1 }] += 1.0;
                }
                let (pa, pb) = (norm_phone(&a.phone), norm_phone(&b.phone));
                if pa.len() >= 7 && pb.len() >= 7 {
                    phone[if pa == pb { 0 } else { 1 }] += 1.0;
                }
            }
        }
        let share = |c: &[f64], i: usize| (c[i] + 0.5) / (c.iter().sum::<f64>() + 0.5 * c.len() as f64);
        let mut said = Vec::new();
        const ENOUGH: f64 = 400.0;
        if name.iter().sum::<f64>() >= ENOUGH {
            m.name_exact.u = share(&name, 0);
            m.name_close.u = share(&name, 1);
            m.name_near.u = share(&name, 2);
            m.name_else.u = share(&name, 3);
            said.push(format!("names over {:.0} pairs", name.iter().sum::<f64>()));
        }
        if email.iter().sum::<f64>() >= ENOUGH {
            m.email_exact.u = share(&email, 0);
            m.email_else.u = share(&email, 1);
            said.push(format!("emails over {:.0}", email.iter().sum::<f64>()));
        }
        if phone.iter().sum::<f64>() >= ENOUGH {
            m.phone_exact.u = share(&phone, 0);
            m.phone_else.u = share(&phone, 1);
            said.push(format!("phones over {:.0}", phone.iter().sum::<f64>()));
        }
        let note = if said.is_empty() {
            "not enough filled-in fields to measure from; the weights are the chosen ones".into()
        } else {
            format!("how often names, emails and phones agree by chance was measured on your list ({})", said.join(", "))
        };
        (m, note)
    }
}

pub fn compare(a: &Contact, b: &Contact, m: &Model) -> Match {
    let mut w = m.prior;
    let mut because = vec![];
    if let Some((lvl, jw)) = name_level(&a.name, &b.name) {
        let level = match lvl {
            0 => {
                because.push("same name".into());
                m.name_exact
            }
            1 => {
                because.push(format!("names {jw:.2} similar"));
                m.name_close
            }
            2 => {
                because.push(format!("names somewhat similar ({jw:.2})"));
                m.name_near
            }
            _ => {
                because.push("different names".into());
                m.name_else
            }
        };
        w += level.weight();
    }
    let (ea, eb) = (norm_email(&a.email), norm_email(&b.email));
    if !ea.is_empty() && !eb.is_empty() {
        if ea == eb {
            because.push("same email".into());
            w += m.email_exact.weight();
        } else {
            because.push("different email".into());
            w += m.email_else.weight();
        }
    }
    let (pa, pb) = (norm_phone(&a.phone), norm_phone(&b.phone));
    if pa.len() >= 7 && pb.len() >= 7 {
        if pa == pb {
            because.push("same phone".into());
            w += m.phone_exact.weight();
        } else {
            because.push("different phone".into());
            w += m.phone_else.weight();
        }
    }
    let probability = 1.0 / (1.0 + 2f64.powf(-w));
    let verdict = if probability >= m.same_above {
        Verdict::Same
    } else if probability >= m.maybe_above {
        Verdict::Maybe
    } else {
        Verdict::Different
    };
    Match { probability, verdict, because }
}

/// Candidate pairs by blocking: only records sharing an email, a phone, or
/// the first three letters of a name token are compared, so a list of
/// thousands does not cost millions of comparisons.
pub fn candidate_pairs(list: &[Contact]) -> Vec<(usize, usize)> {
    let mut blocks: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, c) in list.iter().enumerate() {
        let e = norm_email(&c.email);
        if !e.is_empty() {
            blocks.entry(format!("e:{e}")).or_default().push(i);
        }
        let p = norm_phone(&c.phone);
        if p.len() >= 7 {
            blocks.entry(format!("p:{p}")).or_default().push(i);
        }
        // Three letters, not three bytes: `&tok[..3]` cut "José" or "Zoë"
        // inside a letter and panicked (27 Sep 2026).
        for tok in norm_name(&c.name).split(' ').filter(|t| t.chars().count() >= 3) {
            blocks.entry(format!("n:{}", tok.chars().take(3).collect::<String>())).or_default().push(i);
        }
    }
    let mut pairs = BTreeSet::new();
    for ids in blocks.values() {
        for x in 0..ids.len() {
            for y in x + 1..ids.len() {
                if ids[x] != ids[y] {
                    pairs.insert((ids[x].min(ids[y]), ids[x].max(ids[y])));
                }
            }
        }
    }
    pairs.into_iter().collect()
}

/// Every pair judged Same or Maybe, most likely first.
pub fn duplicates(list: &[Contact], m: &Model) -> Vec<(usize, usize, Match)> {
    let mut out: Vec<(usize, usize, Match)> = candidate_pairs(list)
        .into_iter()
        .map(|(a, b)| (a, b, compare(&list[a], &list[b], m)))
        .filter(|(_, _, r)| r.verdict != Verdict::Different)
        .collect();
    out.sort_by(|x, y| y.2.probability.partial_cmp(&x.2.probability).unwrap_or(std::cmp::Ordering::Equal));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(n: &str, e: &str, p: &str) -> Contact {
        Contact { name: n.into(), email: e.into(), phone: p.into() }
    }

    #[test]
    fn jaro_winkler_reference_values() {
        // Classic published pairs.
        assert!((jaro("martha", "marhta") - 0.9444).abs() < 1e-3);
        assert!((jaro_winkler("martha", "marhta") - 0.9611).abs() < 1e-3);
        assert!((jaro_winkler("dwayne", "duane") - 0.84).abs() < 1e-2);
        assert!((jaro_winkler("dixon", "dicksonx") - 0.8133).abs() < 1e-3);
        assert_eq!(jaro_winkler("", ""), 1.0);
        assert_eq!(jaro_winkler("abc", ""), 0.0);
    }

    #[test]
    fn normalisers() {
        assert_eq!(norm_name("Smith, Dr. John"), "john smith");
        assert_eq!(norm_email("<John.Smith+work@GoogleMail.com>"), "johnsmith@gmail.com");
        assert_eq!(norm_email("john.smith@acme.com"), "john.smith@acme.com");
        assert_eq!(norm_phone("+1 (555) 010-2000"), "5550102000");
    }

    #[test]
    fn same_person_four_ways() {
        let list = vec![
            c("Jon Smith", "jon.smith@acme.com", ""),
            c("Smith, John", "", "(555) 010-2000"),
            c("John Smith", "jon.smith@acme.com", "555-010-2000"),
            c("Priya Raman", "priya@raman.dev", "555-777-1234"),
        ];
        let d = duplicates(&list, &Model::default());
        let pairs: Vec<(usize, usize)> = d.iter().map(|(a, b, _)| (*a, *b)).collect();
        assert!(pairs.contains(&(0, 2)));
        assert!(pairs.contains(&(1, 2)));
        assert!(!pairs.iter().any(|(a, b)| *a == 3 || *b == 3));
        let (_, _, top) = &d[0];
        assert_eq!(top.verdict, Verdict::Same);
        assert!(top.because.iter().any(|b| b.contains("email") || b.contains("phone")));
    }

    #[test]
    fn same_name_different_everything_else_is_not_same() {
        let a = c("John Smith", "john@one.com", "555-111-1111");
        let b = c("John Smith", "jsmith@two.org", "555-222-2222");
        let r = compare(&a, &b, &Model::default());
        assert_ne!(r.verdict, Verdict::Same, "{r:?}");
    }

    #[test]
    fn blocking_skips_unrelated() {
        let list = vec![c("Ann Lee", "", ""), c("Bob Stone", "", "")];
        assert!(candidate_pairs(&list).is_empty());
    }
}
