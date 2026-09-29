//! What leaves the machine, scrubbed: secrets and personal numbers replaced
//! with placeholders before a prompt goes to an online model, and put back in
//! the reply when it returns.
//!
//! **Sources:** the token shapes are the published formats — AWS access key
//! ids (`AKIA`/`ASIA` + 16 base-32 characters), GitHub tokens (`ghp_` … + 36,
//! `github_pat_` + 82), Slack (`xoxb-`/`xoxp-`/`xoxe-`/`xoxa-`), PEM private
//! key blocks, `sk-` API keys, JWTs — as collected by Yelp's
//! `detect-secrets` (Apache-2.0) and `gitleaks` (MIT). The high-entropy rule
//! is detect-secrets' (Shannon entropy over the base-64 alphabet above 4.5,
//! over hex above 3.0). The personal-number checks are the standards': Luhn
//! for card numbers (ISO/IEC 7812), mod-97 for IBANs (ISO 13616), the SSA's
//! never-issued ranges for US SSNs. No regex crate — each shape is a small
//! scanner. Clean-room.
//!
//! **Why Atlas wants it.** The secondary model is the one place a prompt
//! leaves the machine, and a prompt is built from whatever was on screen, in
//! the clipboard, in a file. The project rule is offline first; this is what
//! makes the online second safe to have switched on: the key in a config
//! file, the card number in an email, never reach a server, and the reply
//! still makes sense because the placeholders are swapped back locally.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    PrivateKey,
    AwsKey,
    GithubToken,
    SlackToken,
    ApiKey,
    Jwt,
    HighEntropy,
    Card,
    Iban,
    Ssn,
    Email,
    Phone,
}

impl Kind {
    fn tag(self) -> &'static str {
        match self {
            Kind::PrivateKey => "PRIVATE_KEY",
            Kind::AwsKey => "AWS_KEY",
            Kind::GithubToken => "GITHUB_TOKEN",
            Kind::SlackToken => "SLACK_TOKEN",
            Kind::ApiKey => "API_KEY",
            Kind::Jwt => "JWT",
            Kind::HighEntropy => "SECRET",
            Kind::Card => "CARD",
            Kind::Iban => "IBAN",
            Kind::Ssn => "SSN",
            Kind::Email => "EMAIL",
            Kind::Phone => "PHONE",
        }
    }
    fn say(self) -> &'static str {
        match self {
            Kind::PrivateKey => "private key",
            Kind::AwsKey => "AWS key",
            Kind::GithubToken => "GitHub token",
            Kind::SlackToken => "Slack token",
            Kind::ApiKey => "API key",
            Kind::Jwt => "sign-in token",
            Kind::HighEntropy => "secret-looking string",
            Kind::Card => "card number",
            Kind::Iban => "bank account (IBAN)",
            Kind::Ssn => "social security number",
            Kind::Email => "email address",
            Kind::Phone => "phone number",
        }
    }
}

/// One scrubbing session: the same original always gets the same
/// placeholder, across the system prompt and the user prompt, so the model
/// can refer to "⟦EMAIL_1⟧" consistently and the reply can be put back.
#[derive(Debug, Default)]
pub struct Scrubber {
    found: Vec<(Kind, String, String)>, // kind, original, placeholder
}

fn is_b32(c: u8) -> bool {
    c.is_ascii_uppercase() || (b'2'..=b'7').contains(&c)
}
fn is_tok(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'-'
}
fn is_b64(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'+' || c == b'/' || c == b'=' || c == b'_' || c == b'-'
}

fn entropy(s: &str) -> f64 {
    let mut counts = [0usize; 256];
    for b in s.bytes() {
        counts[b as usize] += 1;
    }
    let n = s.len() as f64;
    counts.iter().filter(|c| **c > 0).map(|c| {
        let p = *c as f64 / n;
        -p * p.log2()
    }).sum()
}

fn luhn(digits: &[u8]) -> bool {
    let mut sum = 0;
    for (i, d) in digits.iter().rev().enumerate() {
        let mut v = (*d - b'0') as u32;
        if i % 2 == 1 {
            v *= 2;
            if v > 9 {
                v -= 9;
            }
        }
        sum += v;
    }
    sum % 10 == 0
}

fn iban_ok(s: &str) -> bool {
    let b = s.as_bytes();
    if !(15..=34).contains(&b.len()) || !b[0].is_ascii_uppercase() || !b[1].is_ascii_uppercase() || !b[2].is_ascii_digit() || !b[3].is_ascii_digit() {
        return false;
    }
    let rearranged = s[4..].bytes().chain(s[..4].bytes());
    let mut rem: u64 = 0;
    for c in rearranged {
        let v = match c {
            b'0'..=b'9' => (c - b'0') as u64,
            b'A'..=b'Z' => (c - b'A') as u64 + 10,
            _ => return false,
        };
        rem = if v >= 10 { (rem * 100 + v) % 97 } else { (rem * 10 + v) % 97 };
    }
    rem == 1
}

/// Spans (start, end, kind) found in `t`, non-overlapping, earliest first.
fn find(t: &str) -> Vec<(usize, usize, Kind)> {
    let b = t.as_bytes();
    let n = b.len();
    let mut out: Vec<(usize, usize, Kind)> = Vec::new();
    let taken = |out: &Vec<(usize, usize, Kind)>, s: usize, e: usize| out.iter().any(|(a, z, _)| s < *z && *a < e);
    let boundary = |i: usize| i == 0 || !is_tok(b[i - 1]);

    // PEM private key blocks, whole.
    let mut from = 0;
    while let Some(i) = t[from..].find("-----BEGIN ") {
        let s = from + i;
        let head_end = t[s..].find('\n').map(|x| s + x).unwrap_or(n);
        if t[s..head_end].contains("PRIVATE KEY-----") {
            let e = t[s..].find("-----END ").and_then(|x| t[s + x..].find("KEY-----").map(|y| s + x + y + 8)).unwrap_or(n);
            out.push((s, e, Kind::PrivateKey));
            from = e;
        } else {
            from = s + 11;
        }
    }

    let mut i = 0;
    while i < n {
        // Byte by byte, but only ever sliced at a character's start: an em
        // dash is three bytes, and slicing into its middle panicked (found by
        // the self-fix path's own test, whose prompt has one).
        if !t.is_char_boundary(i) || !boundary(i) || taken(&out, i, i + 1) {
            i += 1;
            continue;
        }
        let run = |start: usize, ok: fn(u8) -> bool| -> usize {
            let mut e = start;
            while e < n && ok(b[e]) {
                e += 1;
            }
            e
        };
        let rest = &t[i..];
        let mut hit: Option<(usize, Kind)> = None;
        if (rest.starts_with("AKIA") || rest.starts_with("ASIA")) && i + 20 <= n && b[i + 4..i + 20].iter().all(|c| is_b32(*c)) && (i + 20 == n || !is_tok(b[i + 20])) {
            hit = Some((i + 20, Kind::AwsKey));
        } else if rest.starts_with("github_pat_") {
            let e = run(i + 11, |c| c.is_ascii_alphanumeric() || c == b'_');
            if e - (i + 11) >= 82 {
                hit = Some((e, Kind::GithubToken));
            }
        } else if ["ghp_", "gho_", "ghu_", "ghs_", "ghr_"].iter().any(|p| rest.starts_with(p)) {
            let e = run(i + 4, |c| c.is_ascii_alphanumeric());
            if e - (i + 4) >= 36 {
                hit = Some((e, Kind::GithubToken));
            }
        } else if ["xoxb-", "xoxp-", "xoxe-", "xoxa-", "xoxs-"].iter().any(|p| rest.starts_with(p)) {
            let e = run(i + 5, |c| c.is_ascii_alphanumeric() || c == b'-');
            if e - (i + 5) >= 10 {
                hit = Some((e, Kind::SlackToken));
            }
        } else if rest.starts_with("sk-") {
            let e = run(i + 3, is_tok);
            if e - (i + 3) >= 20 {
                hit = Some((e, Kind::ApiKey));
            }
        } else if rest.starts_with("eyJ") {
            let mut e = run(i, |c| is_tok(c) || c == b'.');
            while e > i && b[e - 1] == b'.' {
                e -= 1; // a sentence's full stop is not part of the token
            }
            let parts: Vec<&str> = t[i..e].split('.').collect();
            if parts.len() == 3 && parts[1].starts_with("eyJ") && parts.iter().all(|p| p.len() >= 4) {
                hit = Some((e, Kind::Jwt));
            }
        }
        if let Some((e, k)) = hit {
            out.push((i, e, k));
            i = e;
        } else {
            i += 1;
        }
    }

    // Emails: find '@', grow both ways.
    for (at, _) in t.match_indices('@') {
        let mut s = at;
        while s > 0 && (b[s - 1].is_ascii_alphanumeric() || b"._%+-".contains(&b[s - 1])) {
            s -= 1;
        }
        let mut e = at + 1;
        while e < n && (b[e].is_ascii_alphanumeric() || b".-".contains(&b[e])) {
            e += 1;
        }
        while e > at + 1 && b[e - 1] == b'.' {
            e -= 1;
        }
        let domain = &t[at + 1..e];
        let tld_ok = domain.rsplit('.').next().is_some_and(|x| x.len() >= 2 && x.bytes().all(|c| c.is_ascii_alphabetic()));
        if s < at && domain.contains('.') && tld_ok && !taken(&out, s, e) {
            out.push((s, e, Kind::Email));
        }
    }

    // Digit runs with separators: cards, SSNs, phones.
    let mut i = 0;
    while i < n {
        let starts = b[i].is_ascii_digit() || (b[i] == b'+' && i + 1 < n && b[i + 1].is_ascii_digit()) || (b[i] == b'(' && i + 1 < n && b[i + 1].is_ascii_digit());
        if !starts || (i > 0 && (b[i - 1].is_ascii_alphanumeric())) || taken(&out, i, i + 1) {
            i += 1;
            continue;
        }
        let mut e = i;
        let mut digits: Vec<u8> = Vec::new();
        while e < n && (b[e].is_ascii_digit() || (b" -().+".contains(&b[e]) && e + 1 < n && (b[e + 1].is_ascii_digit() || b" -(".contains(&b[e + 1])))) {
            if b[e].is_ascii_digit() {
                digits.push(b[e]);
            }
            e += 1;
        }
        while e > i && !b[e - 1].is_ascii_digit() && b[e - 1] != b')' {
            e -= 1;
        }
        if e < n && b[e].is_ascii_alphabetic() {
            i = e.max(i + 1);
            continue;
        }
        let span = &t[i..e];
        let kind = if (13..=19).contains(&digits.len()) && luhn(&digits) && !span.contains('(') && !span.starts_with('+') {
            Some(Kind::Card)
        } else if digits.len() == 9 && span.len() == 11 && b[i + 3] == b'-' && b[i + 6] == b'-' {
            let area: u32 = span[0..3].parse().unwrap_or(0);
            let group: u32 = span[4..6].parse().unwrap_or(0);
            let serial: u32 = span[7..11].parse().unwrap_or(0);
            (area != 0 && area != 666 && area < 900 && group != 0 && serial != 0).then_some(Kind::Ssn)
        } else if (span.starts_with('+') && (8..=15).contains(&digits.len()))
            || (digits.len() == 10 && (span.contains('-') || span.contains('(') || span.contains('.') || span.contains(' ')) && span.len() >= 12)
            || (digits.len() == 11 && digits[0] == b'1' && span.len() >= 13)
        {
            Some(Kind::Phone)
        } else {
            None
        };
        if let Some(k) = kind {
            if !taken(&out, i, e) {
                out.push((i, e, k));
            }
        }
        i = e.max(i + 1);
    }

    // IBANs, written with or without the spaces banks print.
    let mut i = 0;
    while i + 4 < n {
        if boundary(i) && b[i].is_ascii_uppercase() && b[i + 1].is_ascii_uppercase() && b[i + 2].is_ascii_digit() && b[i + 3].is_ascii_digit() {
            let mut e = i;
            let mut packed = String::new();
            while e < n && (b[e].is_ascii_alphanumeric() || (b[e] == b' ' && e + 1 < n && b[e + 1].is_ascii_alphanumeric() && packed.len() % 4 == 0)) {
                if b[e] != b' ' {
                    packed.push(b[e].to_ascii_uppercase() as char);
                }
                e += 1;
                if packed.len() >= 34 {
                    break;
                }
            }
            // Try the longest valid prefix (the grouping can swallow a word after it).
            let mut best = None;
            let mut count = 0;
            for (k, c) in t[i..e].char_indices() {
                if c != ' ' {
                    count += 1;
                    if count >= 15 && iban_ok(&packed[..count]) {
                        best = Some(i + k + 1);
                    }
                }
            }
            if let Some(end) = best {
                if !taken(&out, i, end) {
                    out.push((i, end, Kind::Iban));
                    i = end;
                    continue;
                }
            }
        }
        i += 1;
    }

    // High-entropy tokens that nothing above named.
    let mut i = 0;
    while i < n {
        if !is_b64(b[i]) || (i > 0 && is_b64(b[i - 1])) {
            i += 1;
            continue;
        }
        let mut e = i;
        while e < n && is_b64(b[e]) {
            e += 1;
        }
        let tok = &t[i..e];
        let hex = tok.len() >= 32 && tok.bytes().all(|c| c.is_ascii_hexdigit());
        let mixed = tok.bytes().any(|c| c.is_ascii_digit()) && tok.bytes().any(|c| c.is_ascii_alphabetic());
        let pathish = tok.matches('/').count() >= 2 || tok.contains("://");
        let secret = if hex { entropy(tok) > 3.0 } else { tok.len() >= 20 && mixed && !pathish && entropy(tok) > 4.5 };
        if secret && !taken(&out, i, e) {
            out.push((i, e, Kind::HighEntropy));
        }
        i = e;
    }

    out.sort_by_key(|x| x.0);
    out
}

/// The kinds of *secret* in `text` -- keys, tokens, card and account
/// numbers, SSNs, secret-looking strings -- as plain names. Email addresses
/// and phone numbers are personal, not secret, and aren't listed. Round 11:
/// what the clipboard history, the mail cache and the receipts reader check
/// before anything is kept.
pub fn secrets_in(text: &str) -> Vec<&'static str> {
    let mut kinds: Vec<Kind> = find(text).into_iter().map(|(_, _, k)| k).filter(|k| !matches!(k, Kind::Email | Kind::Phone)).collect();
    kinds.sort();
    kinds.dedup();
    kinds.into_iter().map(|k| k.say()).collect()
}

impl Scrubber {
    /// `text` with every secret and personal number replaced by a placeholder.
    pub fn scrub(&mut self, text: &str) -> String {
        let spans = find(text);
        let mut out = String::with_capacity(text.len());
        let mut last = 0;
        for (s, e, k) in spans {
            let original = &text[s..e];
            let ph = match self.found.iter().find(|(_, o, _)| o == original) {
                Some((_, _, p)) => p.clone(),
                None => {
                    let nth = self.found.iter().filter(|(kk, ..)| *kk == k).count() + 1;
                    let p = format!("⟦{}_{nth}⟧", k.tag());
                    self.found.push((k, original.to_string(), p.clone()));
                    p
                }
            };
            out.push_str(&text[last..s]);
            out.push_str(&ph);
            last = e;
        }
        out.push_str(&text[last..]);
        out
    }

    /// The reply with the placeholders swapped back — this happens on the
    /// machine, after the answer has come home.
    pub fn put_back(&self, reply: &str) -> String {
        let mut r = reply.to_string();
        for (_, original, ph) in &self.found {
            r = r.replace(ph, original);
        }
        r
    }

    /// "2 email addresses and 1 AWS key were kept back", or None.
    pub fn say(&self) -> Option<String> {
        if self.found.is_empty() {
            return None;
        }
        let mut kinds: Vec<Kind> = self.found.iter().map(|f| f.0).collect();
        kinds.sort();
        kinds.dedup();
        let parts: Vec<String> = kinds
            .iter()
            .map(|k| {
                let c = self.found.iter().filter(|f| f.0 == *k).count();
                format!("{c} {}{}", k.say(), if c == 1 { "" } else { "s" })
            })
            .collect();
        Some(format!("{} kept back from the online model", parts.join(", ")))
    }
}
