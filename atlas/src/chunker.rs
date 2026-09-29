//! Cut a document into pieces small enough to search and quote, and keep the
//! line numbers so every answer can say where it came from.
//!
//! **Source:** `benbrandt/text-splitter` (MIT) — its idea is a ladder of
//! semantic levels (characters → words → sentences → line breaks), splitting
//! at the *largest* level that fits and packing neighbours up to a maximum,
//! with an optional overlap that must be smaller than the chunk. Clean-room;
//! this version adds what Atlas specifically needs and text-splitter does not
//! carry: **1-based line spans** and the **heading path** of each chunk.
//!
//! **Why Atlas wants it.** Idea #6 is "grounded, cited answers from your own
//! material — citing the source file+line". `recall` stores whole notes. A
//! 40-page document as one piece either matches everything or quotes the wrong
//! paragraph; as chunks with spans, the answer can say `plan.md:112-131`.
//! Chunks never cross a markdown heading, because a chunk that is half one
//! section and half the next cites a place that says neither thing.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub text: String,
    /// 1-based, inclusive.
    pub start_line: usize,
    pub end_line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
    /// "Setup > Windows" — the headings this chunk sits under.
    pub heading: String,
}

impl Chunk {
    /// `notes/plan.md:12-18` (or `:12` for a single line).
    pub fn cite(&self, path: &str) -> String {
        if self.start_line == self.end_line {
            format!("{path}:{}", self.start_line)
        } else {
            format!("{path}:{}-{}", self.start_line, self.end_line)
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ChunkConfig {
    /// Maximum chunk length in bytes of original text.
    pub max: usize,
    /// Bytes of the previous chunk's tail to repeat at the start of the next,
    /// so a sentence cut at a boundary is whole in one of them. Must be < max.
    pub overlap: usize,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        ChunkConfig { max: 1200, overlap: 150 }
    }
}

type Span = (usize, usize);

fn line_starts(text: &str) -> Vec<usize> {
    let mut v = vec![0];
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            v.push(i + 1);
        }
    }
    v
}

fn line_of(starts: &[usize], byte: usize) -> usize {
    match starts.binary_search(&byte) {
        Ok(i) => i + 1,
        Err(i) => i,
    }
}

fn trim_span(text: &str, (a, b): Span) -> Option<Span> {
    let s = &text[a..b];
    let lead = s.len() - s.trim_start().len();
    let trail = s.len() - s.trim_end().len();
    (a + lead < b - trail).then_some((a + lead, b - trail))
}

/// Sections split at markdown headings: (heading_path, span of body).
fn sections(text: &str) -> Vec<(String, Span)> {
    let mut out = vec![];
    let mut path: Vec<(usize, String)> = vec![];
    let mut body_start = 0;
    let mut pos = 0;
    let mut in_fence = false;
    let current = |path: &Vec<(usize, String)>| path.iter().map(|(_, h)| h.as_str()).collect::<Vec<_>>().join(" > ");
    for line in text.split_inclusive('\n') {
        let t = line.trim_start();
        if t.starts_with("```") {
            in_fence = !in_fence;
        }
        let level = t.bytes().take_while(|b| *b == b'#').count();
        if !in_fence && (1..=6).contains(&level) && t[level..].starts_with(' ') {
            out.push((current(&path), (body_start, pos)));
            let title = t[level..].trim().to_string();
            path.retain(|(l, _)| *l < level);
            path.push((level, title));
            body_start = pos; // the heading line belongs to its own section
        }
        pos += line.len();
    }
    out.push((current(&path), (body_start, text.len())));
    out.into_iter().filter(|(_, (a, b))| b > a).collect()
}

fn split_by(text: &str, (a, b): Span, is_break: impl Fn(&str, usize) -> Option<usize>) -> Vec<Span> {
    let mut out = vec![];
    let mut start = a;
    let mut i = a;
    while i < b {
        if let Some(end) = is_break(text, i) {
            let end = end.min(b);
            out.push((start, end));
            start = end;
            i = end;
        } else {
            i += text[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        }
    }
    if start < b {
        out.push((start, b));
    }
    out
}

fn paragraphs(text: &str, span: Span) -> Vec<Span> {
    split_by(text, span, |t, i| {
        (t.as_bytes()[i] == b'\n').then(|| {
            // a blank line (only whitespace) ends the paragraph
            let rest = &t[i + 1..];
            let line_end = rest.find('\n').map(|x| i + 1 + x);
            match line_end {
                Some(e) if t[i + 1..e].trim().is_empty() => Some(e + 1),
                _ => None,
            }
        })
        .flatten()
    })
}

fn sentences(text: &str, span: Span) -> Vec<Span> {
    split_by(text, span, |t, i| {
        let c = t.as_bytes()[i];
        if matches!(c, b'.' | b'!' | b'?' | b'\n') {
            let next = t[i + 1..].chars().next();
            if next.is_none_or(|n| n.is_whitespace()) {
                return Some(i + 1);
            }
        }
        None
    })
}

fn words(text: &str, span: Span) -> Vec<Span> {
    split_by(text, span, |t, i| (t.as_bytes()[i] == b' ').then_some(i + 1))
}

fn chars(text: &str, (a, b): Span, max: usize) -> Vec<Span> {
    let mut out = vec![];
    let mut s = a;
    while s < b {
        let mut e = (s + max).min(b);
        while !text.is_char_boundary(e) {
            e -= 1;
        }
        if e == s {
            e = s + text[s..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        }
        out.push((s, e));
        s = e;
    }
    out
}

/// Break a span into units each no longer than `max`, using the largest
/// semantic level that works.
fn units(text: &str, span: Span, max: usize) -> Vec<Span> {
    let mut out = vec![];
    for p in paragraphs(text, span) {
        if p.1 - p.0 <= max {
            out.push(p);
            continue;
        }
        for s in sentences(text, p) {
            if s.1 - s.0 <= max {
                out.push(s);
                continue;
            }
            for w in words(text, s) {
                if w.1 - w.0 <= max {
                    out.push(w);
                } else {
                    out.extend(chars(text, w, max));
                }
            }
        }
    }
    out
}

pub fn chunk(text: &str, cfg: ChunkConfig) -> Result<Vec<Chunk>, String> {
    if cfg.max == 0 {
        return Err("max must be at least 1".into());
    }
    if cfg.overlap >= cfg.max {
        return Err(format!("overlap {} must be smaller than max {}", cfg.overlap, cfg.max));
    }
    let starts = line_starts(text);
    let mut out = vec![];
    for (heading, sec) in sections(text) {
        let us: Vec<Span> = units(text, sec, cfg.max);
        let mut i = 0;
        while i < us.len() {
            let first = us[i].0;
            let mut j = i;
            while j + 1 < us.len() && us[j + 1].1 - first <= cfg.max {
                j += 1;
            }
            if let Some((a, b)) = trim_span(text, (first, us[j].1)) {
                out.push(Chunk {
                    text: text[a..b].to_string(),
                    start_line: line_of(&starts, a),
                    end_line: line_of(&starts, b.saturating_sub(1).max(a)),
                    start_byte: a,
                    end_byte: b,
                    heading: heading.clone(),
                });
            }
            if j + 1 >= us.len() {
                break;
            }
            // next chunk starts at the earliest unit whose tail fits in overlap
            let mut k = j + 1;
            while k > i + 1 && us[j].1 - us[k - 1].0 <= cfg.overlap {
                k -= 1;
            }
            i = k;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "# Setup\n\nInstall the model. Check the hash.\n\n## Windows\n\nRun atlas.exe once.\nIt prints the command list.\n\n# Trading\n\nKeep a journal. Nothing is automatic yet.\n";

    #[test]
    fn chunks_never_cross_headings_and_carry_the_path() {
        let c = chunk(DOC, ChunkConfig { max: 500, overlap: 0 }).unwrap();
        let heads: Vec<&str> = c.iter().map(|c| c.heading.as_str()).collect();
        assert_eq!(heads, vec!["Setup", "Setup > Windows", "Trading"]);
        assert!(c[1].text.contains("Run atlas.exe"));
        assert!(!c[1].text.contains("journal"));
    }

    #[test]
    fn line_spans_cite_the_right_lines() {
        let c = chunk(DOC, ChunkConfig { max: 500, overlap: 0 }).unwrap();
        // "## Windows" is line 5, "It prints the command list." is line 8
        assert_eq!(c[1].cite("setup.md"), "setup.md:5-8");
        let lines: Vec<&str> = DOC.lines().collect();
        assert!(lines[c[2].end_line - 1].contains("Keep a journal"));
    }

    #[test]
    fn long_paragraph_splits_on_sentences_then_words() {
        let para = "Alpha beta gamma. ".repeat(20);
        let c = chunk(&para, ChunkConfig { max: 60, overlap: 0 }).unwrap();
        assert!(c.len() > 3);
        assert!(c.iter().all(|x| x.text.len() <= 60));
        assert!(c.iter().all(|x| x.text.ends_with('.')), "{c:?}");
        let one_word = "x".repeat(130);
        let c = chunk(&one_word, ChunkConfig { max: 50, overlap: 0 }).unwrap();
        assert_eq!(c.len(), 3);
    }

    #[test]
    fn overlap_repeats_the_tail() {
        let text = "One two three. Four five six. Seven eight nine. Ten eleven twelve.";
        let c = chunk(text, ChunkConfig { max: 48, overlap: 20 }).unwrap();
        assert!(c.len() >= 2);
        for w in c.windows(2) {
            assert!(w[1].start_byte < w[0].end_byte, "no overlap: {w:?}");
        }
        // and it always makes progress
        assert!(c.windows(2).all(|w| w[1].start_byte > w[0].start_byte));
    }

    #[test]
    fn headings_inside_code_fences_are_not_headings() {
        let t = "# Real\n\n```\n# not a heading\n```\nafter\n";
        let c = chunk(t, ChunkConfig::default()).unwrap();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].heading, "Real");
    }

    #[test]
    fn bad_config_refused_and_utf8_safe() {
        assert!(chunk("x", ChunkConfig { max: 10, overlap: 10 }).is_err());
        let t = "héllo wörld ".repeat(30);
        let c = chunk(&t, ChunkConfig { max: 7, overlap: 0 }).unwrap();
        assert!(!c.is_empty());
    }
}
