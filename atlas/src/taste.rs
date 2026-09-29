//! Design taste, the part of it a machine can actually hold to.
//!
//! Taste has no compiler. There is no tool that reads a page and says "this is
//! well designed", the way `cargo check` says "this compiles" — so the honest
//! move is not to pretend otherwise, but to split taste into the part that IS
//! checkable and the part that isn't, and to be rigorous about the first.
//!
//! What *is* checkable is consistency and correctness against a stated house
//! style: is the spacing on the scale, are colours coming from tokens rather
//! than typed-in hex, does every image have alt text, does every control have a
//! name. None of that is "beautiful" — but a page that breaks these reads as
//! careless no matter how good the underlying idea is, and a page that keeps
//! them reads as considered. This module is that checkable part, and nothing
//! more: it is the design equivalent of `craft`'s ladder, a gate a draft can be
//! iterated against, not a claim to judgement it does not have.
//!
//! The part that isn't checkable — is this the right layout, does it feel right
//! — stays where it belongs: with a person, or a stronger model. `review` never
//! speaks to that, and a page that passes every rule here is "consistent and
//! accessible", never "good". Saying more than that would be the same overclaim
//! `build_it` and `selfwork` exist to prevent, one domain over.

use serde::{Deserialize, Serialize};

/// How much a finding matters. The split decides whether the build loop stops
/// to fix it or just mentions it, exactly like `craft::Tells::blocks_later`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Severity {
    /// Breaks a floor the house style will not cross — an inaccessible control,
    /// a colour typed in by hand. Worth stopping to fix.
    Blocking,
    /// Worth changing, not worth blocking on — spacing a hair off the scale, a
    /// heading level skipped. Reported, never a reason to fail.
    Advisory,
}

/// One thing the review found, in words a person can act on rather than a rule
/// id. The tool's own voice, kept honest: it says what and where, never "this
/// is ugly".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub severity: Severity,
    /// The short name of the rule, for grouping — "alt text", "colour token".
    pub rule: String,
    /// What was found and why it matters, in one plain sentence.
    pub detail: String,
}

/// The house style, as the set of rules a page is held to.
///
/// Everything here is a stated preference, not a law of design — which is why
/// it is a config with defaults rather than constants. Change the base unit and
/// the spacing check changes with it; that is the point.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rules {
    /// The spacing grid: lengths should be whole multiples of this many pixels.
    /// Off-grid spacing is the single most common reason a careful-looking
    /// design looks slightly off, and it is entirely mechanical to catch.
    pub base_unit: u32,
    /// Colours belong in tokens (CSS variables), not typed into markup. A hex
    /// literal in an inline style is a colour that can never be themed and will
    /// drift from every other use of "the same" colour.
    pub colours_from_tokens: bool,
    /// Accessibility floors: alt text, a language, a name on every control.
    /// These are the ones that are not taste at all — they are whether the page
    /// works for someone who can't see it — so they are the blocking ones.
    pub accessibility: bool,
    /// Prefer classes to inline styles. Advisory: an inline style here and
    /// there is fine, a page built entirely of them is not.
    pub prefer_classes: bool,
}

impl Default for Rules {
    fn default() -> Self {
        Rules { base_unit: 4, colours_from_tokens: true, accessibility: true, prefer_classes: true }
    }
}

/// Review a page of HTML against the house style.
///
/// Deterministic and offline — plain scanning, no model in the loop — because
/// the whole value of this is that it is the *reliable* half of taste. It reads
/// the markup, not a rendering, so it catches what is in the source: it cannot
/// tell you the contrast is too low once CSS variables resolve, and it does not
/// pretend to. What it catches, it catches every time.
pub fn review(html: &str, rules: &Rules) -> Vec<Finding> {
    let mut out = Vec::new();
    let lower = html.to_lowercase();

    if rules.accessibility {
        // Every <img> needs alt text. An image without it is invisible to a
        // screen reader and shows nothing when it fails to load.
        for tag in tags(&lower, "img") {
            if !has_attr(&tag, "alt") {
                out.push(Finding {
                    severity: Severity::Blocking,
                    rule: "alt text".into(),
                    detail: "an <img> has no alt text — it's invisible to a screen reader and to \
                             anyone whose image didn't load"
                        .into(),
                });
            }
        }
        // The document needs a language.
        if lower.contains("<html") && !tags(&lower, "html").iter().any(|t| has_attr(t, "lang")) {
            out.push(Finding {
                severity: Severity::Blocking,
                rule: "page language".into(),
                detail: "the <html> tag has no lang attribute, so assistive tech can't tell what \
                         language the page is in"
                    .into(),
            });
        }
        // Every form control needs a name a screen reader can announce.
        for kind in ["input", "select", "textarea"] {
            for tag in tags(&lower, kind) {
                let named = has_attr(&tag, "aria-label")
                    || has_attr(&tag, "aria-labelledby")
                    || has_attr(&tag, "id") // a <label for=id> may name it; treated as named
                    || has_attr(&tag, "title");
                if !named {
                    out.push(Finding {
                        severity: Severity::Blocking,
                        rule: "control name".into(),
                        detail: format!(
                            "a <{kind}> has nothing to name it (no label, aria-label or id), so it's \
                             unusable without sight"
                        ),
                    });
                }
            }
        }
    }

    if rules.colours_from_tokens {
        // Hex colours typed into an inline style. `var(--token)` is the right
        // shape; `#3a7bd5` in a style attribute is the wrong one.
        for style in inline_styles(html) {
            if let Some(hex) = first_hex_colour(&style) {
                out.push(Finding {
                    severity: Severity::Blocking,
                    rule: "colour token".into(),
                    detail: format!(
                        "the colour {hex} is typed into an inline style — it should come from a \
                         token (a CSS variable) so it can be themed and stays consistent"
                    ),
                });
            }
        }
    }

    // Spacing off the grid, from inline styles. Advisory: a design reads as
    // considered when its spacing is on one scale, and slightly off otherwise.
    for style in inline_styles(html) {
        for px in pixel_lengths(&style) {
            if rules.base_unit > 0 && px % rules.base_unit != 0 {
                out.push(Finding {
                    severity: Severity::Advisory,
                    rule: "spacing scale".into(),
                    detail: format!(
                        "{px}px is off the {}px spacing grid — the nearest on-grid values are \
                         {}px and {}px",
                        rules.base_unit,
                        px - (px % rules.base_unit),
                        px - (px % rules.base_unit) + rules.base_unit
                    ),
                });
            }
        }
    }

    if rules.prefer_classes {
        let n = inline_styles(html).len();
        // A threshold, not zero: the point is a page built out of inline styles,
        // not the occasional one. Ten is "this is how the page is built".
        if n >= 10 {
            out.push(Finding {
                severity: Severity::Advisory,
                rule: "inline styles".into(),
                detail: format!(
                    "{n} inline style attributes — the page is styled element by element rather \
                     than through classes, which makes it hard to keep consistent"
                ),
            });
        }
    }

    // Heading levels that skip (h1 then h3) — a structure, and a screen-reader,
    // problem, but advisory because it degrades rather than breaks.
    if rules.accessibility {
        if let Some((from, to)) = first_heading_skip(&lower) {
            out.push(Finding {
                severity: Severity::Advisory,
                rule: "heading order".into(),
                detail: format!(
                    "a heading jumps from h{from} to h{to}, skipping a level — it reads as a gap \
                     in the outline"
                ),
            });
        }
    }

    out
}

/// The findings that block, in order. The build loop's cue to iterate.
pub fn blocking(findings: &[Finding]) -> Vec<&Finding> {
    findings.iter().filter(|f| f.severity == Severity::Blocking).collect()
}

/// One line summarising a review, in the tool's honest voice: what it can say,
/// and no more.
pub fn spoken(findings: &[Finding]) -> String {
    let blocking = findings.iter().filter(|f| f.severity == Severity::Blocking).count();
    let advisory = findings.len() - blocking;
    if findings.is_empty() {
        return "Consistent and accessible against the house style — which is the checkable part, \
                not whether it's the right design."
            .into();
    }
    let mut s = String::new();
    if blocking > 0 {
        s.push_str(&format!(
            "{blocking} thing{} to fix before this is ready",
            if blocking == 1 { "" } else { "s" }
        ));
    }
    if advisory > 0 {
        if !s.is_empty() {
            s.push_str(", and ");
        }
        s.push_str(&format!("{advisory} worth a look"));
    }
    s.push('.');
    s
}

// --- generating web output the review can gate -----------------------------
//
// The review above judges a page that already exists. This turns it into a
// gate on a page being *made*: draft the HTML, review it, and if a blocking
// rule fails, hand the model the exact problems and ask for a corrected page,
// up to a budget. It is the same shape as `motion::draw_loop` and
// `explain::explain_loop` — draft, check, fix, and be honest when a problem
// survives — one domain over. The house style it holds the draft to is the
// same `Rules`, so what a person can configure for a review also configures
// what generated pages are held to.

/// The system prompt for drafting a web page. It states the house-style floors
/// as instructions so the first draft usually passes them, rather than relying
/// on the fix rounds to drag it there: self-contained, tokens for colour, a
/// spacing scale, and the accessibility floors.
pub const WEB_SYSTEM: &str = "\
You are writing a single, self-contained HTML page for a personal assistant to \
check and show. Return ONLY the page: one HTML document with <html lang=\"…\">, \
a <head> with a <style> block, and a <body>. Put colours in CSS custom \
properties (variables) in :root and use var(--name) everywhere — never a hex \
colour typed into an inline style. Style through classes in the <style> block, \
not inline style attributes. Use a consistent spacing scale (multiples of a \
small base like 4px or 8px) for margins and padding. Give every <img> an alt \
attribute, every form control a <label> or aria-label, and don't skip heading \
levels. No external files, no <script> unless asked, no explanation before or \
after — just the HTML in a single fenced code block.";

/// The system prompt for correcting a page against named problems. Same shape
/// as `build_it::FIX_SYSTEM`, one medium over: it is handed the problems and
/// the page, and returns the whole corrected page.
pub const WEB_FIX_SYSTEM: &str = "\
You are correcting a single self-contained HTML page. You will be given the \
problems found and the current page. Fix exactly those problems and return the \
COMPLETE corrected HTML — the whole document, not a fragment or a diff — in a \
single fenced code block, with no explanation before or after. Keep everything \
that already worked; change only what the problems name.";

/// What a taste-gated web build came to. Parallels `motion::Outcome`: a page
/// that clears the blocking floors, a page whose problems outlived the budget
/// (handed back WITH them, never as a success), or no usable draft at all.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// It clears every blocking rule. Any remaining findings are advisory notes
    /// — worth a look, not reasons it was held back.
    Built { html: String, rounds: u32, notes: Vec<Finding> },
    /// A blocking problem survived the fix budget. The best draft is handed back
    /// WITH the problems attached, so it is never mistaken for a clean page.
    Struggled { html: String, rounds: u32, findings: Vec<Finding> },
    /// No usable first draft — the model returned nothing, or nothing shaped
    /// like a page.
    NoDraft(String),
}

impl Outcome {
    /// The finished (or best) page, whichever way it went; `None` only when
    /// there was never a draft.
    pub fn html(&self) -> Option<&str> {
        match self {
            Outcome::Built { html, .. } | Outcome::Struggled { html, .. } => Some(html),
            Outcome::NoDraft(_) => None,
        }
    }

    /// One honest line about how it went — consistent-and-accessible when it
    /// cleared the floors, plain about what is still wrong when it didn't, and
    /// careful never to call a clean page "good".
    pub fn spoken(&self) -> String {
        match self {
            Outcome::Built { rounds, notes, .. } => {
                let mut s = String::from(
                    "Made a page that's consistent and accessible against the house style — which is \
                     the checkable part, not whether it's the right design.",
                );
                if *rounds > 0 {
                    s.push_str(&format!(" It took {rounds} pass{} to get there.", if *rounds == 1 { "" } else { "es" }));
                }
                if !notes.is_empty() {
                    s.push_str("\nWorth a look:");
                    for f in notes {
                        s.push_str(&format!("\n  • {}", f.detail));
                    }
                }
                s
            }
            Outcome::Struggled { rounds, findings, .. } => {
                let mut s = format!(
                    "I drafted a page but couldn't get it past the house style in {rounds} pass{} — \
                     here's the best draft, with what's still wrong so you can see it:",
                    if *rounds == 1 { "" } else { "es" }
                );
                for f in blocking(findings) {
                    s.push_str(&format!("\n  • {}", f.detail));
                }
                s
            }
            Outcome::NoDraft(why) => format!("I couldn't get a first draft of the page: {why}"),
        }
    }
}

/// Draft a web page and iterate it against the house style until it clears the
/// blocking rules or runs out of budget.
///
/// The `check` closure is injected — in the daemon it is `|html|
/// review(html, &rules)`, and in tests it is whatever the test wants — so the
/// loop logic is exercised without a model deciding the outcome. Mirrors
/// `motion::draw_loop`: the model drafts and fixes, the deterministic check
/// decides.
pub fn build_web(
    brief: &str,
    llm: &dyn crate::brain::Llm,
    max_rounds: u32,
    mut check: impl FnMut(&str) -> Vec<Finding>,
) -> Outcome {
    let mut html = match draft_page(brief, llm) {
        Ok(h) => h,
        Err(e) => return Outcome::NoDraft(e),
    };
    let mut rounds = 0;
    loop {
        let findings = check(&html);
        let has_blocker = findings.iter().any(|f| f.severity == Severity::Blocking);
        if !has_blocker {
            let notes = findings.into_iter().filter(|f| f.severity == Severity::Advisory).collect();
            return Outcome::Built { html, rounds, notes };
        }
        if rounds >= max_rounds {
            return Outcome::Struggled { html, rounds, findings };
        }
        rounds += 1;
        let problems = findings
            .iter()
            .filter(|f| f.severity == Severity::Blocking)
            .map(|f| f.detail.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        match fix_page(&html, &problems, llm) {
            Ok(h) => html = h,
            Err(e) => {
                let mut findings = findings;
                findings.push(Finding {
                    severity: Severity::Blocking,
                    rule: "fix".into(),
                    detail: format!("couldn't get a fix: {e}"),
                });
                return Outcome::Struggled { html, rounds, findings };
            }
        }
    }
}

fn draft_page(brief: &str, llm: &dyn crate::brain::Llm) -> Result<String, String> {
    let reply = llm.complete(WEB_SYSTEM, brief).map_err(|e| e.to_string())?;
    let html = crate::build_it::extract_code(&reply);
    if !looks_like_page(&html) {
        return Err("the model didn't return anything shaped like an HTML page".into());
    }
    Ok(html)
}

fn fix_page(html: &str, problems: &str, llm: &dyn crate::brain::Llm) -> Result<String, String> {
    let user =
        format!("The problems:\n{problems}\n\nThe page:\n```\n{html}\n```\n\nReturn the complete corrected page.");
    let reply = llm.complete(WEB_FIX_SYSTEM, &user).map_err(|e| e.to_string())?;
    let fixed = crate::build_it::extract_code(&reply);
    if !looks_like_page(&fixed) {
        return Err("no page came back on the fix round".into());
    }
    Ok(fixed)
}

/// Enough of a page to be worth reviewing: it has a tag in it. Deliberately
/// loose — the review, not this, is where the standards live; this only guards
/// against an empty reply or a paragraph of prose.
fn looks_like_page(s: &str) -> bool {
    let l = s.to_lowercase();
    !s.trim().is_empty() && l.contains('<') && (l.contains("<html") || l.contains("<body") || l.contains("<div") || l.contains("<!doctype"))
}

/// Does this build request want a web page, rather than a script or program?
///
/// A word-level check, on the same footing as `build_it::lang_from_words`: if
/// the request names a page, a site, HTML or the web, the taste gate applies.
/// Kept conservative — a false "yes" would route an ordinary script through an
/// HTML reviewer, so it only fires on words that clearly mean the web.
pub fn wants_web_page(description: &str) -> bool {
    let d = description.to_lowercase();
    const WEB_WORDS: &[&str] = &[
        "web page", "webpage", "web site", "website", "html page", "landing page",
        "a page", "html", "a site", "microsite", "home page", "homepage",
    ];
    WEB_WORDS.iter().any(|w| contains_phrase(&d, w))
}

/// Whole-token phrase match, so "html" doesn't fire inside "htmlish" and "a
/// page" needs the word page, not "pager". Word boundaries either side.
fn contains_phrase(haystack: &str, phrase: &str) -> bool {
    let bytes = haystack.as_bytes();
    let mut from = 0;
    while let Some(rel) = haystack[from..].find(phrase) {
        let at = from + rel;
        let before_ok = at == 0 || !bytes[at - 1].is_ascii_alphanumeric();
        let after = at + phrase.len();
        let after_ok = bytes.get(after).map(|c| !c.is_ascii_alphanumeric()).unwrap_or(true);
        if before_ok && after_ok {
            return true;
        }
        from = at + phrase.len();
    }
    false
}

// --- the scanning, kept simple and honest ----------------------------------
//
// Deliberately not a real HTML parser: a parser is a dependency and a parser
// can disagree with a browser. These read the source the way the rules are
// stated against it — attributes present, values typed in — and where they
// can't be sure they don't guess. A missed finding is better than an invented
// one, the same bias the rest of the tree takes.

/// The text of every `<name ...>` opening tag in the html.
fn tags(lower_html: &str, name: &str) -> Vec<String> {
    let open = format!("<{name}");
    let mut out = Vec::new();
    let bytes = lower_html.as_bytes();
    let mut i = 0;
    while let Some(rel) = lower_html[i..].find(&open) {
        let start = i + rel;
        // Must be followed by whitespace, '>' or '/', so `<input` doesn't match
        // `<inputs` and `<a` doesn't match `<article`.
        let after = start + open.len();
        let ok = bytes.get(after).map(|c| *c == b' ' || *c == b'>' || *c == b'/' || *c == b'\n' || *c == b'\t').unwrap_or(false);
        if let Some(end_rel) = lower_html[start..].find('>') {
            if ok {
                out.push(lower_html[start..start + end_rel + 1].to_string());
            }
            i = start + end_rel + 1;
        } else {
            break;
        }
    }
    out
}

/// Does this tag text carry the named attribute (with any value, or bare)?
fn has_attr(tag: &str, attr: &str) -> bool {
    // Look for the attribute name preceded by whitespace and followed by '=',
    // whitespace, '>' or '/'. Avoids `id` matching inside `aria-hidden`.
    let bytes = tag.as_bytes();
    let mut i = 0;
    while let Some(rel) = tag[i..].find(attr) {
        let at = i + rel;
        let before_ok = at == 0 || matches!(bytes[at - 1], b' ' | b'\n' | b'\t' | b'"' | b'\'');
        let after = at + attr.len();
        let after_ok = bytes
            .get(after)
            .map(|c| matches!(c, b'=' | b' ' | b'>' | b'/' | b'\n' | b'\t'))
            .unwrap_or(true);
        if before_ok && after_ok {
            return true;
        }
        i = at + attr.len();
    }
    false
}

/// The contents of every `style="..."` attribute in the (original-case) html.
fn inline_styles(html: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(rel) = lower[i..].find("style=") {
        let at = i + rel + "style=".len();
        let rest = &html[at..];
        let Some(quote) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            i = at;
            continue;
        };
        let body = &rest[1..];
        if let Some(end) = body.find(quote) {
            out.push(body[..end].to_string());
            i = at + 1 + end + 1;
        } else {
            break;
        }
    }
    out
}

/// The first `#rgb`/`#rrggbb` colour literal in a style string, if any.
fn first_hex_colour(style: &str) -> Option<String> {
    let bytes = style.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'#' {
            let hex: String = style[i + 1..]
                .chars()
                .take_while(|c| c.is_ascii_hexdigit())
                .collect();
            if hex.len() == 3 || hex.len() == 6 {
                return Some(format!("#{hex}"));
            }
        }
        i += 1;
    }
    None
}

/// Every `<n>px` length in a style string.
fn pixel_lengths(style: &str) -> Vec<u32> {
    let mut out = Vec::new();
    let bytes = style.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if style[i..].starts_with("px") {
                if let Ok(n) = style[start..i].parse::<u32>() {
                    // 0px and 1px are never "off grid" in a meaningful way.
                    if n > 1 {
                        out.push(n);
                    }
                }
            }
        } else {
            i += 1;
        }
    }
    out
}

/// The first place a heading level jumps by more than one going down the page,
/// as (from, to). Only downward skips (h1→h3), which are the ones that read as
/// a missing level; going back up (h3→h2 in a new section) is normal.
fn first_heading_skip(lower_html: &str) -> Option<(u8, u8)> {
    let mut last: Option<u8> = None;
    let mut i = 0;
    let b = lower_html.as_bytes();
    while let Some(rel) = lower_html[i..].find("<h") {
        let at = i + rel + 2;
        i = at;
        let Some(&c) = b.get(at) else { break };
        if (b'1'..=b'6').contains(&c) {
            let level = c - b'0';
            if let Some(prev) = last {
                if level > prev + 1 {
                    return Some((prev, level));
                }
            }
            last = Some(level);
        }
    }
    None
}
