//! "This is from your installer — isn't it?" Senders that look like someone
//! you deal with and aren't, and mail whose own server says the sender
//! checks failed.
//!
//! **Sources:** Unicode TR #39 (*Unicode Security Mechanisms*) §4, the
//! confusable *skeleton*: map every character to its prototype and compare
//! the results — a subset of `confusables.txt` (Unicode licence) covering the
//! Cyrillic and Greek letters and the ASCII look-alikes (`rn`→`m`, `vv`→`w`,
//! `0`→`o`, `1`/`I`→`l`) that phishing actually uses. `elceef/dnstwist`
//! (Apache-2.0) for the list of domain permutations worth checking: one letter
//! dropped, added, swapped or changed; a different ending; the real name with
//! a word bolted on; the real domain as a subdomain of someone else's. RFC
//! 8601 for the `Authentication-Results` header (`spf=`, `dkim=`, `dmarc=`).
//! Clean-room.
//!
//! **Why Atlas wants it.** The mail check already knows who your clients are
//! (`clients`) and drafts replies to them. That is exactly the list a
//! lookalike is built against: `acme-lnstall.com` replying about an invoice
//! would get a polite, helpful draft. Now it gets a warning instead, and no
//! draft.

/// The TR #39 skeleton, reduced to what matters for addresses and names.
fn skeleton(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(|c| c.to_lowercase()) {
        let m = match c {
            // Cyrillic
            'а' => 'a', 'в' => 'b', 'е' | 'ё' => 'e', 'к' => 'k', 'м' => 'm', 'н' => 'h', 'о' => 'o',
            'р' => 'p', 'с' => 'c', 'т' => 't', 'у' => 'y', 'х' => 'x', 'ѕ' => 's', 'і' | 'ї' => 'i',
            'ј' => 'j', 'ԁ' => 'd', 'ԛ' => 'q', 'ԝ' => 'w', 'һ' => 'h', 'ӏ' => 'l',
            // Greek
            'α' => 'a', 'β' => 'b', 'ε' => 'e', 'η' => 'n', 'ι' => 'i', 'κ' => 'k', 'ν' => 'v',
            'ο' => 'o', 'ρ' => 'p', 'τ' => 't', 'υ' => 'u', 'χ' => 'x', 'ω' => 'w',
            // Latin look-alikes and marks
            'ı' => 'i', 'ɡ' => 'g', 'ʟ' => 'l', 'ᴅ' => 'd', 'ɑ' => 'a',
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a', 'è' | 'é' | 'ê' | 'ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' => 'i', 'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o', 'ù' | 'ú' | 'û' | 'ü' => 'u',
            'ç' => 'c', 'ñ' => 'n', 'ý' | 'ÿ' => 'y',
            // ASCII that reads as other ASCII
            '0' => 'o', '1' | '|' => 'l', '5' => 's',
            // Fullwidth forms
            c @ '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            c => c,
        };
        out.push(m);
    }
    // Sequences that render as one letter. 'i' → 'l' last: lowercase i and
    // l are the classic pair in a sans-serif mail client.
    out.replace("rn", "m").replace("vv", "w").replace("cl", "d").replace('i', "l")
}

fn label_and_suffix(domain: &str) -> (String, String) {
    let d = domain.trim().trim_end_matches('.').to_lowercase();
    let parts: Vec<&str> = d.split('.').collect();
    if parts.len() < 2 {
        return (d, String::new());
    }
    // Two-part public suffixes that matter for a small business's contacts.
    let two = ["co.uk", "com.au", "co.nz", "co.jp", "com.br", "co.za", "org.uk", "ac.uk"];
    let last2 = parts[parts.len() - 2..].join(".");
    if parts.len() >= 3 && two.contains(&last2.as_str()) {
        return (parts[parts.len() - 3].to_string(), last2);
    }
    (parts[parts.len() - 2].to_string(), parts[parts.len() - 1].to_string())
}

fn one_edit_apart(a: &str, b: &str) -> Option<&'static str> {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a == b {
        return None;
    }
    if a.len() == b.len() {
        let diff: Vec<usize> = (0..a.len()).filter(|i| a[*i] != b[*i]).collect();
        if diff.len() == 1 {
            return Some("one letter changed");
        }
        if diff.len() == 2 && diff[1] == diff[0] + 1 && a[diff[0]] == b[diff[1]] && a[diff[1]] == b[diff[0]] {
            return Some("two letters swapped");
        }
        return None;
    }
    let (long, short, what) = if a.len() > b.len() { (&a, &b, "one letter added") } else { (&b, &a, "one letter dropped") };
    if long.len() != short.len() + 1 {
        return None;
    }
    let i = (0..short.len()).find(|i| long[*i] != short[*i]).unwrap_or(short.len());
    (long[i + 1..] == short[i..]).then_some(what)
}

/// Why `domain` looks like `known` without being it, or None.
fn resembles(domain: &str, known: &str) -> Option<String> {
    let d = domain.trim().trim_end_matches('.').to_lowercase();
    let k = known.trim().trim_end_matches('.').to_lowercase();
    if d.is_empty() || k.is_empty() || d == k || d.ends_with(&format!(".{k}")) {
        return None; // the domain itself, or a real subdomain of it
    }
    let (dl, ds) = label_and_suffix(&d);
    let (kl, ks) = label_and_suffix(&k);
    if kl.len() < 4 {
        return None; // three letters is too short to say anything useful about
    }
    if (d.starts_with("xn--") || d.contains(".xn--"))
        && (skeleton(&dl) == skeleton(&kl) || dl.contains(&kl)) {
            return Some(format!("an encoded (xn--) name made to look like {k}"));
        }
    if skeleton(&d) == skeleton(&k) || (skeleton(&dl) == skeleton(&kl) && ds == ks) {
        return Some(format!("made of letters that look like {k} but aren't the same characters"));
    }
    if dl == kl && ds != ks {
        return Some(format!("the same name as {k} with a different ending (.{ds})"));
    }
    if ds == ks {
        if let Some(how) = one_edit_apart(&dl, &kl) {
            return Some(format!("{k} with {how}"));
        }
    }
    if d.starts_with(&format!("{k}.")) {
        return Some(format!("{k} used as the start of someone else's address ({d})"));
    }
    let bolted = [format!("{kl}-"), format!("-{kl}")];
    if dl != kl && (bolted.iter().any(|b| dl.contains(b.as_str())) || (dl.starts_with(&kl) && dl.len() <= kl.len() + 8)) {
        return Some(format!("{k}'s name with something added ({dl})"));
    }
    None
}

/// Did the receiving server vouch that this mail really is from its sender
/// -- a DMARC or DKIM pass? Mail with no such word (no header at all, which
/// is what Himalaya and some providers hand over) is not vouched for: Atlas
/// may draft to it but never sends to it unasked (1 Oct 2026 security pass:
/// a forged "client" mail with no header used to get an automatic reply).
pub fn vouched_for(auth_results: &str) -> bool {
    let c = Checks::parse(auth_results);
    c.says_forged().is_none() && (c.dmarc.as_deref() == Some("pass") || c.dkim.as_deref() == Some("pass"))
}

/// The receiving server's verdicts from `Authentication-Results`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Checks {
    spf: Option<String>,
    dkim: Option<String>,
    dmarc: Option<String>,
}

impl Checks {
    fn parse(header: &str) -> Checks {
        let mut c = Checks::default();
        // RFC 8601: `authserv-id; method=result ...; method=result ...`
        for part in header.split(';').skip(1) {
            let p = part.trim();
            let Some((method, rest)) = p.split_once('=') else { continue };
            let result = rest.split(|ch: char| ch.is_whitespace() || ch == '(').next().unwrap_or("").to_lowercase();
            let slot = match method.trim().to_lowercase().as_str() {
                "spf" => &mut c.spf,
                "dkim" => &mut c.dkim,
                "dmarc" => &mut c.dmarc,
                _ => continue,
            };
            // Several dkim signatures: a pass anywhere counts.
            if slot.as_deref() != Some("pass") {
                *slot = Some(result);
            }
        }
        c
    }

    /// The server said the sender is not who the From line says.
    fn says_forged(&self) -> Option<String> {
        if self.dmarc.as_deref() == Some("fail") {
            return Some("its own mail server marked it as failing the sender check (DMARC)".into());
        }
        let hard = |x: &Option<String>| matches!(x.as_deref(), Some("fail") | Some("softfail"));
        if hard(&self.spf) && self.dkim.as_deref() != Some("pass") {
            return Some("it wasn't sent from a server the sender's domain allows (SPF), and has no valid signature".into());
        }
        None
    }
}

/// One incoming message, judged against the domains you deal with. `from`
/// is the raw From header ("Dana <dana@acme.com>").
pub fn sender_warning(from: &str, auth_results: &str, known_domains: &[String]) -> Option<String> {
    let (name, address) = match (from.rfind('<'), from.rfind('>')) {
        (Some(a), Some(b)) if b > a => (from[..a].trim().trim_matches('"').to_string(), from[a + 1..b].trim().to_lowercase()),
        _ => (String::new(), from.trim().to_lowercase()),
    };
    let domain = address.rsplit_once('@').map(|x| x.1.to_string()).unwrap_or_default();
    let who = if name.is_empty() { address.clone() } else { format!("\"{name}\" <{address}>") };
    if let Some(why) = Checks::parse(auth_results).says_forged() {
        return Some(format!("Careful with the mail from {who}: {why}."));
    }
    if known_domains.iter().any(|k| k.eq_ignore_ascii_case(&domain)) {
        return None;
    }
    for k in known_domains {
        if let Some(why) = resembles(&domain, k) {
            return Some(format!("Careful with the mail from {who}: the address is {why}. Nothing was drafted to it."));
        }
    }
    // A display name that is itself one of your domains, on another address.
    let lname = name.to_lowercase();
    if let Some(k) = known_domains.iter().find(|k| k.len() > 3 && lname.contains(&k.to_lowercase())) {
        return Some(format!("Careful with the mail from {who}: the name says {k} but it came from {domain}."));
    }
    None
}
