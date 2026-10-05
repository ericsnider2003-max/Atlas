//! Research write-ups as Word and PDF files (1 Oct 2026, research report
//! item 24: "researched, cited reports exported to Word or PDF").
//!
//! Atlas's write-ups are Markdown (`research::Research::save`,
//! `writing_help`): a title, the body with headings and lists, and a
//! "Sources" list. This turns one into
//!
//! - a **.docx** that Word, Pages, Google Docs and LibreOffice open: the
//!   title and headings as Word's own heading styles (so the navigation
//!   pane works), bold kept, bullets and numbered points indented, and each
//!   source a working link, numbered so "[2]" in the text can be followed;
//! - a **.pdf**, US Letter, Helvetica, with headings, wrapped text, page
//!   numbers, and each source a clickable link.
//!
//! Neither needs a library: a .docx is a zip of XML (ECMA-376 Part 1 --
//! WordprocessingML §17, packaging Part 2), written here uncompressed; the
//! PDF is ISO 32000-1 text with the standard Helvetica (§9.6.2.2, one of the
//! 14 fonts every reader has, so nothing is embedded) and its published
//! widths for wrapping. Every PDF written is read back by `pdfkit` and its
//! pages counted before it's handed over.
//!
//! Characters outside Windows-1252 can't be drawn by the standard fonts:
//! common punctuation is mapped ("smart" quotes, dashes, bullets, the
//! ellipsis), anything else becomes "?" in the PDF. The .docx keeps
//! everything.

/// A piece of the write-up.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Title(String),
    Heading(String),
    Para(String),
    Bullet(String),
    Numbered(String, String),
    /// A source: its number and its address.
    Source(usize, String),
}

/// The write-up's pieces, from its Markdown.
pub fn write_up_blocks(md: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut para = String::new();
    let mut in_sources = false;
    let mut n = 0;
    let flush = |para: &mut String, out: &mut Vec<Block>| {
        if !para.trim().is_empty() {
            out.push(Block::Para(para.trim().to_string()));
        }
        para.clear();
    };
    for line in md.lines() {
        let l = line.trim();
        if l.is_empty() {
            flush(&mut para, &mut out);
            continue;
        }
        if let Some(h) = l.strip_prefix("# ") {
            flush(&mut para, &mut out);
            out.push(if out.is_empty() { Block::Title(h.trim().to_string()) } else { Block::Heading(h.trim().to_string()) });
            in_sources = false;
        } else if l.starts_with("##") {
            flush(&mut para, &mut out);
            let h = l.trim_start_matches('#').trim().to_string();
            in_sources = h.eq_ignore_ascii_case("sources");
            out.push(Block::Heading(h));
        } else if let Some(item) = l.strip_prefix("- ").or_else(|| l.strip_prefix("* ")).or_else(|| l.strip_prefix("• ")) {
            flush(&mut para, &mut out);
            let item = item.trim();
            if in_sources && (item.starts_with("http://") || item.starts_with("https://")) {
                n += 1;
                out.push(Block::Source(n, item.to_string()));
            } else {
                out.push(Block::Bullet(item.to_string()));
            }
        } else if let Some((num, rest)) = numbered(l) {
            flush(&mut para, &mut out);
            out.push(Block::Numbered(num, rest));
        } else {
            if !para.is_empty() {
                para.push(' ');
            }
            para.push_str(l);
        }
    }
    flush(&mut para, &mut out);
    out
}

fn numbered(l: &str) -> Option<(String, String)> {
    let digits: String = l.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || digits.len() > 3 {
        return None;
    }
    let rest = l[digits.len()..].strip_prefix(". ").or_else(|| l[digits.len()..].strip_prefix(") "))?;
    Some((format!("{digits}."), rest.trim().to_string()))
}

/// Inline Markdown as runs of (text, bold): `**bold**` kept, `*x*`, `_x_`
/// and backticks dropped, `[text](url)` as "text (url)".
fn runs(text: &str) -> Vec<(String, bool)> {
    let mut plain = String::new();
    let mut rest = text;
    // Links first.
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        match (after.find("]("), after.find(')')) {
            (Some(close), Some(end)) if close < end => {
                let label = &after[..close];
                let url = &after[close + 2..end];
                plain.push_str(&rest[..open]);
                plain.push_str(label);
                if !url.is_empty() && url != label {
                    plain.push_str(&format!(" ({url})"));
                }
                rest = &after[end + 1..];
            }
            _ => {
                plain.push_str(&rest[..open + 1]);
                rest = after;
            }
        }
    }
    plain.push_str(rest);
    let mut out: Vec<(String, bool)> = Vec::new();
    for (i, part) in plain.split("**").enumerate() {
        let cleaned: String = part.replace('`', "");
        let cleaned = strip_emphasis(&cleaned);
        if cleaned.is_empty() {
            continue;
        }
        let bold = i % 2 == 1;
        match out.last_mut() {
            Some((t, b)) if *b == bold => t.push_str(&cleaned),
            _ => out.push((cleaned, bold)),
        }
    }
    out
}

/// `*word*` and `_word_` at word edges lose their marks; a lone `*` or an
/// underscore inside a word (snake_case) stays.
fn strip_emphasis(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let starts = w.starts_with(['*', '_']);
            let core = w.trim_end_matches(|c: char| c.is_ascii_punctuation() && c != '*' && c != '_');
            let ends = core.ends_with(['*', '_']);
            if starts || ends {
                w.chars().filter(|c| *c != '*').collect::<String>().trim_start_matches('_').replace("_.", ".").replace("_,", ",").trim_end_matches('_').to_string()
            } else {
                w.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn plain(text: &str) -> String {
    runs(text).into_iter().map(|(t, _)| t).collect()
}

// ======================================================= .docx

fn xml_text(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            c if (c as u32) < 0x20 && c != '\t' => {}
            c => o.push(c),
        }
    }
    o
}

fn w_runs(text: &str) -> String {
    runs(text)
        .into_iter()
        .map(|(t, bold)| format!("<w:r>{}<w:t xml:space=\"preserve\">{}</w:t></w:r>", if bold { "<w:rPr><w:b/></w:rPr>" } else { "" }, xml_text(&t)))
        .collect()
}

/// The write-up as a .docx file.
pub fn docx(md: &str) -> Vec<u8> {
    let mut body = String::new();
    let mut links: Vec<String> = Vec::new();
    for b in write_up_blocks(md) {
        match b {
            Block::Title(t) => body.push_str(&format!("<w:p><w:pPr><w:pStyle w:val=\"Title\"/></w:pPr>{}</w:p>", w_runs(&t))),
            Block::Heading(t) => body.push_str(&format!("<w:p><w:pPr><w:pStyle w:val=\"Heading1\"/></w:pPr>{}</w:p>", w_runs(&t))),
            Block::Para(t) => body.push_str(&format!("<w:p>{}</w:p>", w_runs(&t))),
            Block::Bullet(t) => body.push_str(&format!(
                "<w:p><w:pPr><w:ind w:left=\"720\" w:hanging=\"360\"/></w:pPr><w:r><w:t xml:space=\"preserve\">\u{2022}\t</w:t></w:r>{}</w:p>",
                w_runs(&t)
            )),
            Block::Numbered(n, t) => body.push_str(&format!(
                "<w:p><w:pPr><w:ind w:left=\"720\" w:hanging=\"360\"/></w:pPr><w:r><w:t xml:space=\"preserve\">{}\t</w:t></w:r>{}</w:p>",
                xml_text(&n),
                w_runs(&t)
            )),
            Block::Source(n, url) => {
                links.push(url.clone());
                body.push_str(&format!(
                    "<w:p><w:pPr><w:ind w:left=\"720\" w:hanging=\"360\"/></w:pPr><w:r><w:t xml:space=\"preserve\">[{n}]\t</w:t></w:r>\
                     <w:hyperlink r:id=\"rLink{}\"><w:r><w:rPr><w:rStyle w:val=\"Hyperlink\"/></w:rPr><w:t>{}</w:t></w:r></w:hyperlink></w:p>",
                    links.len(),
                    xml_text(&url)
                ));
            }
        }
    }
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
         xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><w:body>{body}\
         <w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/><w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" \
         w:header=\"720\" w:footer=\"720\" w:gutter=\"0\"/></w:sectPr></w:body></w:document>"
    );
    let mut rels = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
         <Relationship Id=\"rStyles\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>",
    );
    for (i, url) in links.iter().enumerate() {
        rels.push_str(&format!(
            "<Relationship Id=\"rLink{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"{}\" TargetMode=\"External\"/>",
            i + 1,
            xml_text(url)
        ));
    }
    rels.push_str("</Relationships>");
    let style = |id: &str, name: &str, size: u32, bold: bool, color: &str, after: u32| {
        format!(
            "<w:style w:type=\"paragraph\" w:styleId=\"{id}\"><w:name w:val=\"{name}\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/>\
             <w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before=\"240\" w:after=\"{after}\"/>{}</w:pPr><w:rPr>{}<w:color w:val=\"{color}\"/><w:sz w:val=\"{size}\"/></w:rPr></w:style>",
            if id == "Heading1" { "<w:outlineLvl w:val=\"0\"/>" } else { "" },
            if bold { "<w:b/>" } else { "" }
        )
    };
    let styles = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
         <w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"Calibri\" w:hAnsi=\"Calibri\" w:cs=\"Calibri\"/><w:sz w:val=\"22\"/></w:rPr></w:rPrDefault>\
         <w:pPrDefault><w:pPr><w:spacing w:after=\"160\" w:line=\"276\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>\
         <w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:qFormat/></w:style>\
         {}{}\
         <w:style w:type=\"character\" w:styleId=\"Hyperlink\"><w:name w:val=\"Hyperlink\"/><w:rPr><w:color w:val=\"0563C1\"/><w:u w:val=\"single\"/></w:rPr></w:style>\
         </w:styles>",
        style("Title", "Title", 48, false, "1F3864", 240),
        style("Heading1", "heading 1", 30, true, "2F5496", 120)
    );
    let types = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
         <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
         <Default Extension=\"xml\" ContentType=\"application/xml\"/>\
         <Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
         <Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/>\
         </Types>";
    let root_rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
         <Relationship Id=\"rDoc\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>\
         </Relationships>";
    stored_zip(&[
        ("[Content_Types].xml", types.as_bytes()),
        ("_rels/.rels", root_rels.as_bytes()),
        ("word/document.xml", document.as_bytes()),
        ("word/styles.xml", styles.as_bytes()),
        ("word/_rels/document.xml.rels", rels.as_bytes()),
    ])
}

/// A zip with every file stored as is (method 0): what Office needs, and
/// nothing to compress with.
fn stored_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        let crc = crate::zipread::crc32(data);
        let offset = out.len() as u32;
        let head = |sig: &[u8], central: bool| {
            let mut h = sig.to_vec();
            if central {
                h.extend(20u16.to_le_bytes()); // made by
            }
            h.extend(20u16.to_le_bytes()); // needed
            h.extend(0u16.to_le_bytes()); // flags
            h.extend(0u16.to_le_bytes()); // stored
            h.extend(0u16.to_le_bytes()); // time
            h.extend(0x21u16.to_le_bytes()); // date: 1 Jan 1980
            h.extend(crc.to_le_bytes());
            h.extend((data.len() as u32).to_le_bytes());
            h.extend((data.len() as u32).to_le_bytes());
            h.extend((name.len() as u16).to_le_bytes());
            h.extend(0u16.to_le_bytes()); // extra
            if central {
                h.extend(0u16.to_le_bytes()); // comment
                h.extend(0u16.to_le_bytes()); // disk
                h.extend(0u16.to_le_bytes()); // internal attrs
                h.extend(0u32.to_le_bytes()); // external attrs
                h.extend(offset.to_le_bytes());
            }
            h.extend(name.as_bytes());
            h
        };
        out.extend(head(b"PK\x03\x04", false));
        out.extend_from_slice(data);
        central.extend(head(b"PK\x01\x02", true));
    }
    let at = out.len() as u32;
    let size = central.len() as u32;
    out.extend(central);
    out.extend(b"PK\x05\x06");
    out.extend([0u8; 4]);
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((files.len() as u16).to_le_bytes());
    out.extend(size.to_le_bytes());
    out.extend(at.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out
}

// ======================================================= .pdf

/// Helvetica's widths (thousandths of the type size), ' ' to '~', from
/// Adobe's published metrics.
const HELVETICA: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278,
    278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722,
    667, 944, 667, 667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556,
    556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];

/// A character as a Windows-1252 byte (the standard fonts' encoding), and
/// its width.
fn win(c: char) -> (u8, u16) {
    match c {
        ' '..='~' => (c as u8, HELVETICA[c as usize - 32]),
        '\u{2018}' => (0x91, 222),
        '\u{2019}' => (0x92, 222),
        '\u{201C}' => (0x93, 333),
        '\u{201D}' => (0x94, 333),
        '\u{2022}' => (0x95, 350),
        '\u{2013}' => (0x96, 556),
        '\u{2014}' => (0x97, 1000),
        '\u{2026}' => (0x85, 1000),
        '\u{20AC}' => (0x80, 556),
        '\u{A0}' => (b' ', 278),
        c if ('\u{A1}'..='\u{FF}').contains(&c) => (c as u32 as u8, 556),
        _ => (b'?', 556),
    }
}

fn width(s: &str, size: f64, bold: bool) -> f64 {
    let w: u32 = s.chars().map(|c| u32::from(win(c).1)).sum();
    // Helvetica-Bold runs about 6% wider; measured generously so a line
    // never runs past the margin.
    w as f64 * size / 1000.0 * if bold { 1.07 } else { 1.0 }
}

fn pdf_str(s: &str) -> String {
    let mut o = String::from("(");
    for c in s.chars() {
        let b = win(c).0;
        match b {
            b'(' | b')' | b'\\' => {
                o.push('\\');
                o.push(b as char);
            }
            0x20..=0x7E => o.push(b as char),
            _ => o.push_str(&format!("\\{b:03o}")),
        }
    }
    o.push(')');
    o
}

const PAGE_W: f64 = 612.0;
const PAGE_H: f64 = 792.0;
const MARGIN: f64 = 72.0;

struct Page {
    ops: String,
    links: Vec<(f64, f64, f64, f64, String)>,
}

struct Layout {
    pages: Vec<Page>,
    y: f64,
}

impl Layout {
    fn new() -> Layout {
        Layout { pages: vec![Page { ops: String::new(), links: Vec::new() }], y: PAGE_H - MARGIN }
    }
    fn room(&mut self, needed: f64) {
        if self.y - needed < MARGIN {
            self.pages.push(Page { ops: String::new(), links: Vec::new() });
            self.y = PAGE_H - MARGIN;
        }
    }
    /// Words wrapped to the width from `left`, each line `size` high.
    fn text(&mut self, text_runs: &[(String, bool)], size: f64, left: f64, lead: Option<&str>, link: Option<&str>) {
        let line_h = size * 1.35;
        let right = PAGE_W - MARGIN;
        // (word, bold) in order, spaces between.
        let words: Vec<(String, bool)> =
            text_runs.iter().flat_map(|(t, b)| t.split_whitespace().map(move |w| (w.to_string(), *b))).collect();
        let mut lines: Vec<Vec<(String, bool)>> = vec![Vec::new()];
        let mut x = left;
        let space = width(" ", size, false);
        for (w, b) in words {
            let ww = width(&w, size, b);
            if !lines.last().is_some_and(|l| l.is_empty()) && x + space + ww > right {
                lines.push(Vec::new());
                x = left;
            }
            if !lines.last().is_some_and(|l| l.is_empty()) {
                x += space;
            }
            // A word longer than the line (a long address): broken where it must.
            if ww > right - left {
                let mut piece = String::new();
                for c in w.chars() {
                    if width(&format!("{piece}{c}"), size, b) > right - x && !piece.is_empty() {
                        lines.last_mut().unwrap_or(&mut Vec::new()).push((std::mem::take(&mut piece), b));
                        lines.push(Vec::new());
                        x = left;
                    }
                    piece.push(c);
                }
                x += width(&piece, size, b);
                if let Some(l) = lines.last_mut() {
                    l.push((piece, b));
                }
                continue;
            }
            x += ww;
            if let Some(l) = lines.last_mut() {
                l.push((w, b));
            }
        }
        for (i, line) in lines.iter().enumerate() {
            self.room(line_h);
            self.y -= line_h;
            let base = self.y + size * 0.3;
            // `room` above always leaves a page to draw on.
            let Some(page) = self.pages.last_mut() else { continue };
            if i == 0 {
                if let Some(lead) = lead {
                    page.ops.push_str(&format!("BT /F1 {size} Tf {:.2} {base:.2} Td {} Tj ET\n", left - 18.0, pdf_str(lead)));
                }
            }
            let mut x = left;
            let mut first = true;
            for (w, b) in line {
                if !first {
                    x += space;
                }
                first = false;
                let colour = if link.is_some() { "0.02 0.39 0.76 rg " } else { "" };
                page.ops.push_str(&format!("BT {colour}/{} {size} Tf {x:.2} {base:.2} Td {} Tj ET\n", if *b { "F2" } else { "F1" }, pdf_str(w)));
                x += width(w, size, *b);
            }
            if let Some(url) = link {
                page.links.push((left, base - size * 0.25, x, base + size, url.to_string()));
            }
        }
    }
    fn gap(&mut self, h: f64) {
        self.y -= h;
    }
}

/// The write-up as a PDF file.
pub fn pdf(md: &str) -> Result<Vec<u8>, String> {
    let mut l = Layout::new();
    for b in write_up_blocks(md) {
        match b {
            Block::Title(t) => {
                l.text(&[(plain(&t), true)], 20.0, MARGIN, None, None);
                l.gap(10.0);
            }
            Block::Heading(t) => {
                l.gap(8.0);
                l.room(14.0 * 1.35 * 3.0);
                l.text(&[(plain(&t), true)], 14.0, MARGIN, None, None);
                l.gap(2.0);
            }
            Block::Para(t) => {
                l.text(&runs(&t), 11.0, MARGIN, None, None);
                l.gap(7.0);
            }
            Block::Bullet(t) => {
                l.text(&runs(&t), 11.0, MARGIN + 18.0, Some("\u{2022}"), None);
                l.gap(3.0);
            }
            Block::Numbered(n, t) => {
                l.text(&runs(&t), 11.0, MARGIN + 18.0, Some(&n), None);
                l.gap(3.0);
            }
            Block::Source(n, url) => {
                l.text(&[(url.clone(), false)], 10.0, MARGIN + 22.0, Some(&format!("[{n}]")), Some(&url));
                l.gap(3.0);
            }
        }
    }
    let count = l.pages.len();
    // Objects: 1 catalog, 2 pages, 3 Helvetica, 4 Helvetica-Bold, then for
    // each page: the page, its contents, and its links.
    let mut objs: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        String::new(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>".into(),
    ];
    let mut kids = Vec::new();
    for (i, page) in l.pages.iter().enumerate() {
        let footer = format!("Page {} of {count}", i + 1);
        let ops = format!(
            "{}BT 0.45 g /F1 9 Tf {:.2} 40 Td {} Tj ET\n",
            page.ops,
            PAGE_W / 2.0 - width(&footer, 9.0, false) / 2.0,
            pdf_str(&footer)
        );
        let page_no = objs.len() + 1;
        let contents_no = page_no + 1;
        let mut annots = Vec::new();
        let mut link_objs = Vec::new();
        for (j, (x0, y0, x1, y1, url)) in page.links.iter().enumerate() {
            annots.push(format!("{} 0 R", contents_no + 1 + j));
            link_objs.push(format!(
                "<< /Type /Annot /Subtype /Link /Rect [{x0:.2} {y0:.2} {x1:.2} {y1:.2}] /Border [0 0 0] /A << /S /URI /URI {} >> >>",
                pdf_str(url)
            ));
        }
        objs.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_W} {PAGE_H}] /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents {contents_no} 0 R{} >>",
            if annots.is_empty() { String::new() } else { format!(" /Annots [{}]", annots.join(" ")) }
        ));
        objs.push(format!("<< /Length {} >>\nstream\n{ops}endstream", ops.len()));
        objs.extend(link_objs);
        kids.push(format!("{page_no} 0 R"));
    }
    objs[1] = format!("<< /Type /Pages /Kids [{}] /Count {count} >>", kids.join(" "));
    let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    crate::pdfkit::check_written(&out, count)?;
    Ok(out)
}

/// Which kind of file was asked for: "as a Word document", "a PDF".
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Word,
    Pdf,
}

impl Kind {
    fn extension(self) -> &'static str {
        match self {
            Kind::Word => "docx",
            Kind::Pdf => "pdf",
        }
    }
}

/// "Save the report as a Word document", "make that research a PDF",
/// "export the brief to Word", "send me the report as a pdf": the kind.
pub fn file_asked(said: &str) -> Option<Kind> {
    let t = said.trim().to_lowercase();
    let words: Vec<&str> = t.split(|c: char| !c.is_alphanumeric() && c != '-').filter(|w| !w.is_empty()).collect();
    let has = |w: &str| words.contains(&w);
    // A file of yours ("convert invoice.docx to a pdf", "merge these pdfs",
    // "this photo as a pdf") is the file tools' job, not this.
    if t.split_whitespace().any(|w| w.trim_end_matches(['.', '?', '!', ',']).contains('.'))
        || ["file", "files", "pdfs", "photo", "photos", "picture", "image", "scan", "page", "pages", "folder"].iter().any(|w| has(w))
    {
        return None;
    }
    let writeup = ["report", "research", "brief", "write-up", "writeup", "summary", "letter", "essay", "memo"].iter().any(|w| has(w))
        || t.contains("write up");
    let that = ["save", "export", "make", "turn", "convert", "put", "send", "give"].iter().any(|w| words.first() == Some(w) || (words.first().is_some_and(|f| ["can", "could", "please", "atlas"].contains(f)) && words.iter().take(4).any(|x| x == w)))
        && (has("that") || has("it"));
    if !writeup && !that {
        return None;
    }
    let pdf = has("pdf");
    let word = has("docx") || t.contains("word doc") || t.contains("word file") || ["to word", "in word", "as word", "for word", "a word"].iter().any(|p| t.contains(p));
    match (word, pdf) {
        (true, false) => Some(Kind::Word),
        (false, true) => Some(Kind::Pdf),
        _ => None,
    }
}

/// Write the write-up at `md_path` beside itself as the asked kind, and
/// say where.
pub fn export(md_path: &std::path::Path, kind: Kind) -> Result<std::path::PathBuf, String> {
    let md = std::fs::read_to_string(md_path).map_err(|e| format!("I couldn't read the write-up ({e})"))?;
    let bytes = match kind {
        Kind::Word => docx(&md),
        Kind::Pdf => pdf(&md)?,
    };
    let out = md_path.with_extension(kind.extension());
    std::fs::write(&out, bytes).map_err(|e| format!("I couldn't save it ({e})"))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emphasis_marks_go_and_bold_stays() {
        assert_eq!(runs("a **big** deal"), vec![("a ".into(), false), ("big".into(), true), (" deal".into(), false)]);
        assert_eq!(plain("an *odd* `code` and_snake"), "an odd code and_snake");
        assert_eq!(plain("see [the site](https://x.org) now"), "see the site (https://x.org) now");
    }

    #[test]
    fn numbered_points() {
        assert_eq!(numbered("2. Second"), Some(("2.".into(), "Second".into())));
        assert_eq!(numbered("2026 was a year"), None);
    }

    #[test]
    fn characters_the_font_has() {
        assert_eq!(win('\u{2019}').0, 0x92);
        assert_eq!(win('é').0, 0xE9);
        assert_eq!(win('\u{4E2D}').0, b'?');
        assert_eq!(pdf_str("a (b) \\"), "(a \\(b\\) \\\\)");
    }

    #[test]
    fn what_counts_as_asking() {
        for (said, want) in [
            ("save the report as a Word document", Some(Kind::Word)),
            ("make that a PDF", Some(Kind::Pdf)),
            ("export the research to Word", Some(Kind::Word)),
            ("can you save it as a pdf", Some(Kind::Pdf)),
            ("send me the brief as a docx", Some(Kind::Word)),
            ("convert invoice.docx to a pdf", None),
            ("merge these pdfs", None),
            ("save this photo as a pdf", None),
            ("what's a good word for happy", None),
            ("make a pdf of my resume file", None),
        ] {
            assert_eq!(file_asked(said), want, "{said}");
        }
    }
}
