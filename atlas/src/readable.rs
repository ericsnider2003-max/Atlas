//! Pull the article out of a web page and leave the menus, cookie banners,
//! "related stories" and footers behind.
//!
//! **Source:** Mozilla Readability (`mozilla/readability`, Apache-2.0) — the
//! Firefox Reader View algorithm; `kumabook/readability` (MIT, a Rust port)
//! read as a reference. Clean-room. The heuristics kept are the ones that
//! carry the result:
//!
//! * drop "unlikely candidates" by class/id (comment, sidebar, footer, ad,
//!   banner, social…) unless they also look like content (article, main, body);
//! * score each paragraph-ish block of 25+ characters as
//!   `1 + (commas + 1) + min(len/100, 3)`, credit its parent fully, its
//!   grandparent by half, and further ancestors by `1/(level·3)`, up to 5;
//! * seed a candidate by tag (div +5, pre/td/blockquote +3, lists −3,
//!   headings −5) and by class/id (±25);
//! * multiply by `1 − link density`, take the best, and pull in siblings that
//!   score at least `max(10, 0.2·best)` or are long, link-poor paragraphs.
//!
//! **Why Atlas wants it.** `research::strip_html` removes script/style/svg and
//! the tags, then keeps *all* the text — so a research answer is built from
//! the navigation menu, the cookie notice and twelve "you may also like"
//! headlines as much as from the article. This returns the article.

use std::collections::HashMap;

#[derive(Debug, Default, Clone)]
struct El {
    tag: String,
    class_id: String,
    children: Vec<usize>,
    text: String, // for text nodes (tag == "#text")
    parent: Option<usize>,
    href: bool,
    /// Not shown on the page: `hidden`, `aria-hidden="true"`, or a style of
    /// `display:none` / `visibility:hidden` (Readability's
    /// `_isProbablyVisible`). A collapsed menu or a translation left in the
    /// markup for later is not part of what the page says.
    hidden: bool,
}

/// Is this tag hidden from view? Read off its own attributes only.
fn hidden_tag(inner: &str) -> bool {
    if attr(inner, "aria-hidden").is_some_and(|v| v.trim().eq_ignore_ascii_case("true")) {
        return true;
    }
    if let Some(style) = attr(inner, "style") {
        let s: String = style.to_ascii_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
        if s.contains("display:none") || s.contains("visibility:hidden") {
            return true;
        }
    }
    // The bare `hidden` attribute, with every quoted value taken out first so
    // `class="menu hidden"` isn't mistaken for it.
    let mut bare = String::with_capacity(inner.len());
    let mut quote: Option<char> = None;
    for c in inner.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None => bare.push(c),
        }
    }
    bare.split(|c: char| c.is_whitespace() || c == '/')
        .skip(1)
        .any(|t| t.eq_ignore_ascii_case("hidden") || t.to_ascii_lowercase().starts_with("hidden="))
}

const VOID: [&str; 14] = ["br", "hr", "img", "input", "meta", "link", "area", "base", "col", "embed", "source", "track", "wbr", "param"];
const DROP: [&str; 8] = ["script", "style", "noscript", "svg", "iframe", "template", "canvas", "object"];

fn decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let end = tail.find(';').filter(|e| *e <= 10);
        match end {
            Some(e) => {
                let ent = &tail[1..e];
                let ch = match ent {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" | "#39" => Some('\''),
                    "nbsp" => Some(' '),
                    "mdash" => Some('—'),
                    "ndash" => Some('–'),
                    "hellip" => Some('…'),
                    "rsquo" => Some('’'),
                    "lsquo" => Some('‘'),
                    "rdquo" => Some('”'),
                    "ldquo" => Some('“'),
                    _ if ent.starts_with("#x") || ent.starts_with("#X") => {
                        u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32)
                    }
                    _ if ent.starts_with('#') => ent[1..].parse().ok().and_then(char::from_u32),
                    _ => None,
                };
                match ch {
                    Some(c) => {
                        out.push(c);
                        rest = &tail[e + 1..];
                    }
                    None => {
                        out.push('&');
                        rest = &tail[1..];
                    }
                }
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn attr(tag_src: &str, name: &str) -> Option<String> {
    let low = tag_src.to_ascii_lowercase();
    let mut from = 0;
    while let Some(p) = low[from..].find(name) {
        let at = from + p;
        let before_ok = at == 0 || low.as_bytes()[at - 1].is_ascii_whitespace();
        let after = low[at + name.len()..].trim_start();
        if before_ok && after.starts_with('=') {
            let v = after[1..].trim_start();
            let off = tag_src.len() - v.len();
            let raw = &tag_src[off..];
            return Some(match raw.chars().next() {
                Some(q @ ('"' | '\'')) => raw[1..].split(q).next().unwrap_or("").to_string(),
                _ => raw.split(|c: char| c.is_whitespace() || c == '>').next().unwrap_or("").to_string(),
            });
        }
        from = at + name.len();
    }
    None
}

struct Doc {
    els: Vec<El>,
    title: String,
}

fn parse(html: &str) -> Doc {
    let mut els = vec![El { tag: "#root".into(), ..Default::default() }];
    let mut stack = vec![0usize];
    let mut title = String::new();
    let mut i = 0;
    let b = html.as_bytes();
    while i < b.len() {
        if b[i] == b'<' {
            if html[i..].starts_with("<!--") {
                i = html[i..].find("-->").map(|e| i + e + 3).unwrap_or(b.len());
                continue;
            }
            let Some(end) = html[i..].find('>').map(|e| i + e) else { break };
            let inner = &html[i + 1..end];
            i = end + 1;
            if inner.starts_with('!') || inner.starts_with('?') {
                continue;
            }
            if let Some(name) = inner.strip_prefix('/') {
                let name = name.trim().to_ascii_lowercase();
                if let Some(pos) = stack.iter().rposition(|x| els[*x].tag == name) {
                    if pos > 0 {
                        stack.truncate(pos);
                    }
                }
                continue;
            }
            let name: String = inner
                .split(|c: char| c.is_whitespace() || c == '/')
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            if name.is_empty() {
                continue;
            }
            if DROP.contains(&name.as_str()) || name == "title" {
                let close = format!("</{name}");
                let low = html[i..].to_ascii_lowercase();
                let stop = low.find(&close).map(|e| i + e).unwrap_or(b.len());
                if name == "title" && title.is_empty() {
                    title = decode(html[i..stop].trim());
                }
                i = html[stop..].find('>').map(|e| stop + e + 1).unwrap_or(b.len());
                continue;
            }
            // implicit close: a new block closes an open <p>
            if matches!(name.as_str(), "p" | "div" | "ul" | "ol" | "table" | "h1" | "h2" | "h3" | "section" | "article") {
                if let Some(&top) = stack.last() {
                    if els[top].tag == "p" {
                        stack.pop();
                    }
                }
            }
            let parent = *stack.last().unwrap();
            let class_id = format!("{} {}", attr(inner, "class").unwrap_or_default(), attr(inner, "id").unwrap_or_default())
                .to_lowercase();
            let id = els.len();
            let hidden = hidden_tag(inner) && !matches!(name.as_str(), "html" | "body" | "main" | "article");
            els.push(El { tag: name.clone(), class_id, parent: Some(parent), href: name == "a", hidden, ..Default::default() });
            els[parent].children.push(id);
            if !VOID.contains(&name.as_str()) && !inner.ends_with('/') {
                stack.push(id);
            } else if name == "br" {
                let t = els.len();
                els.push(El { tag: "#text".into(), text: "\n".into(), parent: Some(parent), ..Default::default() });
                els[parent].children.push(t);
            }
        } else {
            let end = html[i..].find('<').map(|e| i + e).unwrap_or(b.len());
            let mut text = decode(&html[i..end]);
            // Whitespace between tags is still a word break: `</span> <span>`
            // is "a = b", not "a=b". Kept as one space, or as a line break
            // inside <pre>, where lines are the point.
            if text.trim().is_empty() && !text.is_empty() {
                let in_pre = stack.iter().any(|x| els[*x].tag == "pre");
                text = if in_pre && text.contains('\n') { "\n".into() } else { " ".into() };
            }
            if !text.is_empty() {
                let parent = *stack.last().unwrap();
                let id = els.len();
                els.push(El { tag: "#text".into(), text, parent: Some(parent), ..Default::default() });
                els[parent].children.push(id);
            }
            i = end;
        }
    }
    Doc { els, title }
}

/// How deep Markdown is built before the rest of a subtree is plain text.
const MD_DEPTH: usize = 64;

/// Tags that sit inside a line of text rather than making a new one.
const INLINE: [&str; 17] = ["span", "a", "code", "em", "strong", "b", "i", "u", "sub", "sup", "small", "mark", "abbr", "kbd", "var", "samp", "q"];

impl Doc {
    /// Does this subtree hold the page's own `<main>` or `<article>`? Then it
    /// is not furniture, whatever its class says: MDN wraps its whole page in
    /// `layout__2-sidebars-inline`, and "sidebar" used to throw the article
    /// away with the sidebars.
    fn holds_main(&self, id: usize) -> bool {
        self.els[id].children.iter().any(|c| matches!(self.els[*c].tag.as_str(), "main" | "article") || self.holds_main(*c))
    }
    /// Inside the page's `<main>` or `<article>`, a class word like "header"
    /// names the article's own header (MDN's intro lives in
    /// `layout__header`), so only the tags that are always furniture go.
    fn inside_main(&self, id: usize) -> bool {
        let mut p = self.els[id].parent;
        while let Some(a) = p {
            if matches!(self.els[a].tag.as_str(), "main" | "article") {
                return true;
            }
            p = self.els[a].parent;
        }
        false
    }
    fn text(&self, id: usize) -> String {
        let e = &self.els[id];
        if e.tag == "#text" {
            return e.text.clone();
        }
        let mut s = String::new();
        for c in &e.children {
            s.push_str(&self.text(*c));
            if matches!(self.els[*c].tag.as_str(), "p" | "div" | "li" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "tr" | "pre" | "blockquote" | "section") {
                s.push('\n');
            } else if self.els[*c].tag != "#text" && !INLINE.contains(&self.els[*c].tag.as_str()) {
                // A space between blocks, so "cell" and "cell" don't glue —
                // but not around inline tags: code highlighted token by token
                // in <span>s read as "line . split ()" (found on a real page,
                // docs.python.org, 23 Sep).
                s.push(' ');
            }
        }
        s
    }
    /// One line of inline Markdown: everything under `id`, line breaks
    /// taken out.
    fn md_line(&self, id: usize, depth: usize) -> String {
        self.md_at(id, depth).split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// The children of `id` as one line of inline Markdown: a heading's or
    /// a list item's own words.
    fn inner_line(&self, id: usize, depth: usize) -> String {
        let mut s = String::new();
        for c in &self.els[id].children {
            s.push_str(&self.md_at(*c, depth));
            if !INLINE.contains(&self.els[*c].tag.as_str()) && self.els[*c].tag != "#text" {
                s.push(' ');
            }
        }
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// The subtree as Markdown. Blocks are set apart by blank lines; inline
    /// tags add nothing but inline code's backticks.
    fn md(&self, id: usize) -> String {
        self.md_at(id, 0)
    }

    /// `md`, `depth` levels down. Past `MD_DEPTH` the rest of a subtree is
    /// its plain text: a page whose tags never close (a real one did, on
    /// MDN) nests hundreds deep, and each level of Markdown costs a stack
    /// frame that plain text doesn't.
    fn md_at(&self, id: usize, depth: usize) -> String {
        let e = &self.els[id];
        if e.tag == "#text" {
            return e.text.clone();
        }
        if depth > MD_DEPTH {
            return self.text(id);
        }
        let depth = depth + 1;
        let inner = |sep: &str| -> String {
            let mut s = String::new();
            for c in &e.children {
                s.push_str(&self.md_at(*c, depth));
                if !INLINE.contains(&self.els[*c].tag.as_str()) && self.els[*c].tag != "#text" {
                    s.push_str(sep);
                }
            }
            s
        };
        match e.tag.as_str() {
            h @ ("h1" | "h2" | "h3" | "h4" | "h5" | "h6") => {
                let n: usize = h[1..].parse().unwrap_or(2);
                format!("\n\n{} {}\n\n", "#".repeat(n), self.inner_line(id, depth))
            }
            "ul" | "ol" => {
                let ordered = e.tag == "ol";
                let mut s = String::from("\n\n");
                let mut n = 0;
                for c in &e.children {
                    if self.els[*c].tag != "li" {
                        continue;
                    }
                    n += 1;
                    let item = self.inner_line(*c, depth);
                    if item.is_empty() {
                        continue;
                    }
                    if ordered {
                        s.push_str(&format!("{n}. {item}\n"));
                    } else {
                        s.push_str(&format!("- {item}\n"));
                    }
                }
                s.push('\n');
                s
            }
            "li" => format!("\n- {}\n", self.inner_line(id, depth)),
            "pre" => format!("\n\n```\n{}\n```\n\n", self.text(id).trim_matches('\n')),
            "code" => {
                let t = self.text(id);
                if t.contains('\n') {
                    format!("\n\n```\n{}\n```\n\n", t.trim_matches('\n'))
                } else if t.trim().is_empty() {
                    String::new()
                } else {
                    format!("`{}`", t.trim())
                }
            }
            "blockquote" => {
                let body = tidy_md(&inner("\n"));
                let quoted: Vec<String> = body.lines().map(|l| if l.is_empty() { ">".to_string() } else { format!("> {l}") }).collect();
                format!("\n\n{}\n\n", quoted.join("\n"))
            }
            "tr" => {
                let cells: Vec<String> = e
                    .children
                    .iter()
                    .filter(|c| matches!(self.els[**c].tag.as_str(), "td" | "th"))
                    .map(|c| self.md_line(*c, depth))
                    .collect();
                format!("{}\n", cells.join(" | "))
            }
            "table" | "thead" | "tbody" | "tfoot" => format!("\n\n{}\n\n", inner("")),
            "p" | "div" | "section" | "article" | "main" | "header" | "figure" | "figcaption" | "dl" | "dd" | "dt"
            | "address" | "details" | "summary" => format!("\n\n{}\n\n", inner(" ")),
            "img" | "hr" => String::new(),
            _ => inner(" "),
        }
    }

    fn link_text_len(&self, id: usize) -> usize {
        let e = &self.els[id];
        if e.href {
            return self.text(id).trim().chars().count();
        }
        e.children.iter().map(|c| self.link_text_len(*c)).sum()
    }
    fn link_density(&self, id: usize) -> f64 {
        let total = self.text(id).trim().chars().count();
        if total == 0 {
            return 0.0;
        }
        self.link_text_len(id) as f64 / total as f64
    }
    fn unlikely(&self, id: usize) -> bool {
        let ci = &self.els[id].class_id;
        if ci.trim().is_empty() || matches!(self.els[id].tag.as_str(), "body" | "a" | "html") {
            return false;
        }
        const UNLIKELY: [&str; 26] = [
            "-ad-", "banner", "breadcrumb", "combx", "comment", "community", "cookie", "disqus", "extra", "footer", "gdpr",
            "header", "legends", "menu", "related", "remark", "replies", "rss", "shoutbox", "sidebar", "skyscraper",
            "social", "sponsor", "popup", "pagination", "newsletter",
        ];
        const MAYBE: [&str; 6] = ["and", "article", "body", "column", "content", "main"];
        UNLIKELY.iter().any(|w| ci.contains(w)) && !MAYBE.iter().any(|w| ci.contains(w))
            || matches!(self.els[id].tag.as_str(), "nav" | "footer" | "aside" | "form")
    }
    fn class_weight(&self, id: usize) -> f64 {
        let ci = &self.els[id].class_id;
        let mut w = 0.0;
        const NEG: [&str; 16] = [
            "-ad-", "hidden", "banner", "combx", "comment", "com-", "contact", "footer", "gdpr", "masthead", "meta",
            "promo", "related", "share", "sidebar", "widget",
        ];
        const POS: [&str; 12] = ["article", "body", "content", "entry", "hentry", "h-entry", "main", "page", "post", "text", "blog", "story"];
        if NEG.iter().any(|x| ci.contains(x)) {
            w -= 25.0;
        }
        if POS.iter().any(|x| ci.contains(x)) {
            w += 25.0;
        }
        w
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Article {
    pub title: String,
    pub text: String,
    /// The same article as Markdown: headings as `#`, lists as `-` and `1.`,
    /// quotations as `>`, code in fences, table rows as `a | b`. Links are
    /// kept as their words only -- the addresses are noise to a reader.
    pub markdown: String,
    /// How much of the page's text this kept, 0..1 — a very low number with a
    /// short text usually means the page is not an article (a search page, an
    /// index), and the caller should fall back to the plain strip.
    pub kept_share: f64,
}

pub fn extract(html: &str) -> Article {
    // Readability.js's retry: first with unlikely-looking subtrees removed;
    // if that keeps too little (a class rule caught the content), again with
    // only the tags that are always furniture (nav, footer, aside, form).
    let first = extract_with(html, true);
    if first.text.chars().count() >= 250 {
        return first;
    }
    let second = extract_with(html, false);
    if second.text.chars().count() > first.text.chars().count() {
        second
    } else {
        first
    }
}

fn extract_with(html: &str, strip_unlikely: bool) -> Article {
    let mut d = parse(html);
    // remove unlikely subtrees
    let ids: Vec<usize> = (1..d.els.len()).collect();
    for id in ids {
        let furniture = matches!(d.els[id].tag.as_str(), "nav" | "footer" | "aside" | "form");
        let by_class_only = !furniture && (!strip_unlikely || d.inside_main(id));
        let unseen = d.els[id].hidden;
        if d.els[id].tag != "#text" && (unseen || (d.unlikely(id) && !by_class_only && !d.holds_main(id))) {
            if let Some(p) = d.els[id].parent {
                d.els[p].children.retain(|c| *c != id);
            }
        }
    }
    let total_text = d.text(0).split_whitespace().count().max(1);
    let mut score: HashMap<usize, f64> = HashMap::new();
    let seed = |d: &Doc, id: usize| -> f64 {
        let base = match d.els[id].tag.as_str() {
            "div" | "article" | "main" | "section" => 5.0,
            "pre" | "td" | "blockquote" => 3.0,
            "address" | "ol" | "ul" | "dl" | "dd" | "dt" | "li" | "form" => -3.0,
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "th" => -5.0,
            _ => 0.0,
        };
        base + d.class_weight(id)
    };
    // walk reachable nodes only
    let mut reach = vec![];
    let mut st = vec![0usize];
    while let Some(n) = st.pop() {
        reach.push(n);
        st.extend(d.els[n].children.iter().copied());
    }
    for &id in &reach {
        if !matches!(d.els[id].tag.as_str(), "p" | "td" | "pre" | "section" | "h2" | "h3" | "h4" | "h5" | "h6") {
            continue;
        }
        let t = d.text(id);
        let t = t.trim();
        let len = t.chars().count();
        if len < 25 {
            continue;
        }
        let s = 1.0 + (t.matches([',', '，', '、']).count() + 1) as f64 + ((len / 100).min(3)) as f64;
        let mut anc = d.els[id].parent;
        let mut level = 0;
        while let Some(a) = anc {
            if level >= 5 || a == 0 {
                break;
            }
            let div = match level {
                0 => 1.0,
                1 => 2.0,
                _ => level as f64 * 3.0,
            };
            let e = score.entry(a).or_insert_with(|| seed(&d, a));
            *e += s / div;
            anc = d.els[a].parent;
            level += 1;
        }
    }
    let best = score
        .iter()
        .map(|(id, s)| (*id, s * (1.0 - d.link_density(*id))))
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal).then(b.0.cmp(&a.0)));
    let title = if d.title.is_empty() {
        reach.iter().find(|i| d.els[**i].tag == "h1").map(|i| d.text(*i).trim().to_string()).unwrap_or_default()
    } else {
        d.title.clone()
    };
    let Some((top, top_score)) = best else {
        let text = tidy(&d.text(0));
        let markdown = tidy_md(&d.md(0));
        return Article { title, kept_share: 1.0, text, markdown };
    };
    // siblings
    let parent = d.els[top].parent.unwrap_or(0);
    let threshold = (top_score * 0.2).max(10.0);
    let mut parts = vec![];
    let mut md_parts = vec![];
    for &sib in &d.els[parent].children {
        let keep = if sib == top {
            true
        } else if let Some(s) = score.get(&sib) {
            s * (1.0 - d.link_density(sib)) >= threshold
        } else if d.els[sib].tag == "p" {
            let t = d.text(sib);
            let len = t.trim().chars().count();
            let ld = d.link_density(sib);
            (len > 80 && ld < 0.25) || (len > 0 && len <= 80 && ld == 0.0 && t.contains(". "))
        } else {
            false
        };
        if keep {
            parts.push(d.text(sib));
            md_parts.push(d.md(sib));
        }
    }
    let text = tidy(&parts.join("\n"));
    let markdown = tidy_md(&md_parts.join("\n\n"));
    let kept_share = text.split_whitespace().count() as f64 / total_text as f64;
    Article { title, text, markdown, kept_share }
}

/// Markdown lines tidied: spaces collapsed outside code fences, at most one
/// blank line in a row, none at either end.
fn tidy_md(s: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut in_code = false;
    for line in s.lines() {
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            out.push("```".into());
            continue;
        }
        if in_code {
            out.push(line.trim_end().to_string());
            continue;
        }
        let l = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if l.is_empty() && out.last().map(|p| p.is_empty()).unwrap_or(true) {
            continue;
        }
        // A marker with nothing after it ("-", "#", ">") is an empty block.
        if matches!(l.as_str(), "-" | ">" | "#" | "##" | "###" | "####" | "#####" | "######") {
            continue;
        }
        out.push(l);
    }
    while out.last().map(|l| l.is_empty()).unwrap_or(false) {
        out.pop();
    }
    out.join("\n")
}

fn tidy(s: &str) -> String {
    s.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r#"<!doctype html><html><head><title>Fed holds rates &amp; signals patience</title>
<style>.x{}</style><script>var tracking = "Home News Sport";</script></head>
<body>
<nav class="menu"><a href="/">Home</a> <a href="/news">News</a> <a href="/sport">Sport</a></nav>
<div id="cookie-banner">We use cookies, to improve, your experience, and for ads, analytics, partners.</div>
<header class="site-header"><a href="/">The Daily Example</a></header>
<div class="article-body">
  <h1>Fed holds rates</h1>
  <p>The central bank kept its policy rate unchanged on Wednesday, citing inflation that has cooled, but not as fast as officials had hoped.</p>
  <p>Officials said they would watch incoming data, including payrolls, wages, and consumer prices, before deciding on any change.</p>
  <p>Markets had priced a hold, so the reaction in currencies was muted; the dollar slipped against the yen.</p>
</div>
<aside class="sidebar"><h3>Related</h3><ul><li><a href="/a">Ten stocks to buy now, experts say, before it is too late</a></li></ul></aside>
<div class="related-stories"><p><a href="/b">You may also like: celebrity chef opens restaurant, fans queue, for hours</a></p></div>
<footer><p>© 2026 The Daily Example. All rights reserved, forever, and ever.</p></footer>
</body></html>"#;

    #[test]
    fn keeps_the_article_drops_the_furniture() {
        let a = extract(PAGE);
        assert_eq!(a.title, "Fed holds rates & signals patience");
        assert!(a.text.contains("kept its policy rate unchanged"));
        assert!(a.text.contains("the dollar slipped against the yen"));
        for junk in ["cookies", "Home", "Sport", "Ten stocks", "celebrity chef", "rights reserved", "tracking"] {
            assert!(!a.text.contains(junk), "kept '{junk}':\n{}", a.text);
        }
        assert!(a.kept_share < 0.9);
    }

    #[test]
    fn entities_and_unclosed_paragraphs() {
        let a = extract("<div class=content><p>First para is long enough, clearly, to count here.<p>Second &#8212; also long enough, by far, to be kept.</div>");
        assert!(a.text.contains("First para"));
        assert!(a.text.contains("Second — also"));
    }

    #[test]
    fn no_article_falls_back_to_everything() {
        let a = extract("<html><body><span>just a short line</span></body></html>");
        assert_eq!(a.text, "just a short line");
    }

    #[test]
    fn attribute_reader() {
        assert_eq!(attr(r#"div class="a b" id='main'"#, "class").as_deref(), Some("a b"));
        assert_eq!(attr(r#"div data-class="x" class=y"#, "class").as_deref(), Some("y"));
        assert_eq!(attr("div", "id"), None);
    }
}
