//! Reading the words out of a PDF, in Atlas's own code (Eric's ruling H3,
//! 25 Sep 2026: "read this PDF" is a basic ask).
//!
//! Windows has no PDF-to-text program, and asking you to install one to read
//! a letter is the wrong way round. A PDF is objects and streams: the pages
//! say which fonts and which content streams they use, the content streams
//! draw strings with `Tj`/`TJ`, and a font's `ToUnicode` map says which letter
//! each code is. That is enough for what Word, Google Docs, browsers and
//! scanners' "searchable PDF" produce, which is what arrives in mail.
//!
//! What it does not do: encrypted PDFs (said, not guessed), and fonts with no
//! `ToUnicode` map and a custom encoding (the text comes out as whatever
//! Latin-1 makes of the codes, and `looks_like_words` catches that).
//!
//! A scanned PDF is photos of pages with little or no text. Those photos are
//! nearly always JPEGs, stored as they are, so `images` hands them back
//! untouched for the word reader to read (`files::pdf_is_really_a_scan`
//! decides when).

use std::collections::HashMap;

/// What came out of a PDF.
#[derive(Debug, Default, Clone)]
pub struct Pdf {
    pub pages: usize,
    /// The text, page by page, a blank line between pages.
    pub text: String,
    /// The JPEG photos inside it, in the order they appear. For a scan, these
    /// are the pages.
    pub images: Vec<Vec<u8>>,
}

impl Pdf {
    /// Letters and digits in the text, the measure a scan is told by.
    pub fn text_chars(&self) -> usize {
        self.text.chars().filter(|c| c.is_alphanumeric()).count()
    }
}

/// One object: its dictionary text and, if it has one, its stream (decoded).
#[derive(Debug, Default, Clone)]
struct Obj {
    dict: String,
    stream: Option<Vec<u8>>,
    /// The stream is a JPEG, kept as it was stored.
    jpeg: bool,
}

/// Read a PDF's text and photos.
pub fn read(bytes: &[u8]) -> Result<Pdf, String> {
    if !bytes.starts_with(b"%PDF") {
        return Err("that isn't a PDF".into());
    }
    let mut objs = objects(bytes);
    if objs.values().any(|o| o.dict.contains("/Encrypt")) || trailer_has(bytes, b"/Encrypt") {
        return Err("it's locked with a password, so I can't read it".into());
    }
    unpack_object_streams(&mut objs);

    // Pages, in document order: the page tree from the root, or, failing that,
    // every page object in number order.
    let mut page_ids = page_order(&objs);
    if page_ids.is_empty() {
        let mut ids: Vec<u32> = objs
            .iter()
            .filter(|(_, o)| is_type(&o.dict, "Page"))
            .map(|(id, _)| *id)
            .collect();
        ids.sort();
        page_ids = ids;
    }

    let mut pdf = Pdf { pages: page_ids.len(), ..Default::default() };
    let mut pages_text = Vec::new();
    for id in &page_ids {
        let Some(page) = objs.get(id) else { continue };
        let fonts = fonts_of(&page.dict, &objs, *id);
        let mut text = String::new();
        for c in contents_of(&page.dict) {
            if let Some(Obj { stream: Some(s), .. }) = objs.get(&c) {
                text.push_str(&text_of(s, &fonts));
            }
        }
        pages_text.push(tidy(&text));
    }
    pdf.text = pages_text.join("\n\n").trim().to_string();

    let mut img_ids: Vec<u32> = objs.iter().filter(|(_, o)| o.jpeg).map(|(id, _)| *id).collect();
    img_ids.sort();
    pdf.images = img_ids.into_iter().filter_map(|id| objs.get(&id).and_then(|o| o.stream.clone())).collect();
    Ok(pdf)
}

/// Does the text read as words, or as the garbage an unmapped font makes?
pub fn looks_like_words(text: &str) -> bool {
    let total = text.chars().filter(|c| !c.is_whitespace()).count();
    if total == 0 {
        return false;
    }
    let good = text.chars().filter(|c| c.is_alphanumeric() || ".,;:!?'\"()-%$€£/&@".contains(*c)).count();
    good * 100 / total >= 85
}

// ---------------------------------------------------------------- objects

fn trailer_has(bytes: &[u8], what: &[u8]) -> bool {
    let tail = &bytes[bytes.len().saturating_sub(2048)..];
    find(tail, b"trailer").is_some_and(|i| find(&tail[i..], what).is_some())
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Every `N G obj … endobj` in the file. A later object with the same number
/// replaces an earlier one, which is how incremental saves work.
fn objects(bytes: &[u8]) -> HashMap<u32, Obj> {
    let mut out = HashMap::new();
    let mut i = 0;
    while let Some(k) = find(&bytes[i..], b" obj") {
        let at = i + k;
        i = at + 4;
        let Some(id) = number_before(bytes, at) else { continue };
        let body_start = at + 4;
        let Some(end) = find(&bytes[body_start..], b"endobj") else { break };
        let body = &bytes[body_start..body_start + end];
        out.insert(id, parse_obj(body));
        i = body_start + end + 6;
    }
    out
}

/// `12 0` before ` obj` → 12.
fn number_before(bytes: &[u8], at: usize) -> Option<u32> {
    let head = &bytes[at.saturating_sub(24)..at];
    let s = String::from_utf8_lossy(head);
    let mut parts = s.split_whitespace().rev();
    let gen = parts.next()?;
    let id = parts.next()?;
    gen.parse::<u32>().ok()?;
    let id: String = id.chars().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
    id.parse().ok()
}

fn parse_obj(body: &[u8]) -> Obj {
    let Some(s) = find(body, b"stream") else {
        return Obj { dict: spaced(&String::from_utf8_lossy(body)), ..Default::default() };
    };
    // `endstream` also contains `stream`; the first match is the start.
    let dict = spaced(&String::from_utf8_lossy(&body[..s]));
    let mut start = s + 6;
    if body.get(start) == Some(&b'\r') {
        start += 1;
    }
    if body.get(start) == Some(&b'\n') {
        start += 1;
    }
    let end = find(&body[start..], b"endstream").map(|e| start + e).unwrap_or(body.len());
    let mut raw = &body[start..end];
    if let Some(n) = direct_int(&dict, "/Length") {
        if n <= raw.len() {
            raw = &raw[..n];
        }
    }
    let filters = filters_of(&dict);
    let jpeg = filters.last().is_some_and(|f| f == "DCTDecode") && dict.contains("/Image");
    let stream = decode(raw, &filters);
    Obj { dict, stream, jpeg }
}

/// A dictionary with a space around every delimiter, so `<</F1 5 0 R/F2 6 0 R>>`
/// splits into the same tokens as the tidily written kind.
fn spaced(dict: &str) -> String {
    dict.replace("<<", " << ").replace(">>", " >> ").replace('/', " /").replace('[', " [ ").replace(']', " ] ")
}

fn filters_of(dict: &str) -> Vec<String> {
    let Some(i) = dict.find("/Filter") else { return vec![] };
    let rest = &dict[i + 7..];
    let rest = rest.trim_start();
    // By character: `/Filter` last in the dictionary left nothing to skip,
    // and a byte the lossy decoding turned into a 3-byte replacement
    // character split mid-character (both panicked; found 5 Oct 2026).
    let span = if rest.starts_with('[') { &rest[..rest.find(']').unwrap_or(rest.len())] } else {
        let n = rest.char_indices().skip(1).find(|&(_, c)| c == '/' || c == '>' || c.is_whitespace()).map_or(rest.len(), |(i, _)| i);
        &rest[..n]
    };
    span.split('/').skip(1).map(|f| f.trim_matches(|c: char| c.is_whitespace() || c == ']').to_string()).collect()
}

fn decode(raw: &[u8], filters: &[String]) -> Option<Vec<u8>> {
    let mut data = raw.to_vec();
    for f in filters {
        data = match f.as_str() {
            "FlateDecode" | "Fl" => miniz_oxide::inflate::decompress_to_vec_zlib(&data)
                .or_else(|_| miniz_oxide::inflate::decompress_to_vec(&data))
                .ok()?,
            // Kept as it is: the photo is the JPEG itself.
            "DCTDecode" | "DCT" => data,
            "ASCIIHexDecode" | "AHx" => hex_bytes(&String::from_utf8_lossy(&data)),
            "ASCII85Decode" | "A85" => ascii85(&data)?,
            _ => return None,
        };
    }
    Some(data)
}

/// ASCII85, which reportlab and PostScript-born PDFs wrap streams in.
fn ascii85(data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut group: Vec<u32> = Vec::with_capacity(5);
    let body = data.strip_prefix(b"<~").unwrap_or(data);
    for &c in body {
        match c {
            b'~' => break,
            b'z' if group.is_empty() => out.extend_from_slice(&[0, 0, 0, 0]),
            b'!'..=b'u' => {
                group.push((c - b'!') as u32);
                if group.len() == 5 {
                    let v = group.iter().fold(0u64, |a, d| a * 85 + *d as u64);
                    out.extend_from_slice(&(v as u32).to_be_bytes());
                    group.clear();
                }
            }
            _ if c.is_ascii_whitespace() => {}
            _ => return None,
        }
    }
    if !group.is_empty() {
        let n = group.len();
        while group.len() < 5 {
            group.push(84);
        }
        let v = group.iter().fold(0u64, |a, d| a * 85 + *d as u64);
        out.extend_from_slice(&(v as u32).to_be_bytes()[..n - 1]);
    }
    Some(out)
}

/// `/Length 42` → 42. Not `/Length 7 0 R`, which is a reference.
fn direct_int(dict: &str, key: &str) -> Option<usize> {
    let i = dict.find(key)?;
    let rest = dict[i + key.len()..].trim_start();
    let n: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    let after = rest[n.len()..].trim_start();
    if after.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    n.parse().ok()
}

/// `/Key 12 0 R` → 12.
fn ref_of(dict: &str, key: &str) -> Option<u32> {
    let mut from = 0;
    while let Some(k) = dict[from..].find(key) {
        let i = from + k + key.len();
        from = i;
        // `/Font` must not match `/FontDescriptor`.
        if dict[i..].starts_with(|c: char| c.is_alphanumeric()) {
            continue;
        }
        let rest = dict[i..].trim_start();
        let mut it = rest.split_whitespace();
        let (a, b, c) = (it.next()?, it.next()?, it.next()?);
        if c.starts_with('R') && b.parse::<u32>().is_ok() {
            return a.parse().ok();
        }
        return None;
    }
    None
}

fn is_type(dict: &str, t: &str) -> bool {
    let d = dict.replace(char::is_whitespace, "");
    d.contains(&format!("/Type/{t}/")) || d.contains(&format!("/Type/{t}>")) || d.ends_with(&format!("/Type/{t}"))
}

/// Objects packed inside `/Type /ObjStm` streams, which modern PDFs use for
/// fonts, pages and most dictionaries.
fn unpack_object_streams(objs: &mut HashMap<u32, Obj>) {
    let packed: Vec<(String, Vec<u8>)> = objs
        .values()
        .filter(|o| is_type(&o.dict, "ObjStm"))
        .filter_map(|o| o.stream.clone().map(|s| (o.dict.clone(), s)))
        .collect();
    for (dict, data) in packed {
        let (Some(n), Some(first)) = (direct_int(&dict, "/N"), direct_int(&dict, "/First")) else { continue };
        let head = String::from_utf8_lossy(&data[..first.min(data.len())]).into_owned();
        let nums: Vec<usize> = head.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        for k in 0..n {
            let (Some(&id), Some(&off)) = (nums.get(2 * k), nums.get(2 * k + 1)) else { break };
            let start = first + off;
            let end = nums.get(2 * k + 3).map_or(data.len(), |o| first + o);
            if start >= data.len() || end > data.len() || start > end {
                continue;
            }
            objs.entry(id as u32)
                .or_insert_with(|| Obj { dict: spaced(&String::from_utf8_lossy(&data[start..end])), ..Default::default() });
        }
    }
}

/// Pages in reading order, walking the page tree from the catalogue.
fn page_order(objs: &HashMap<u32, Obj>) -> Vec<u32> {
    let Some(root) = objs.iter().find(|(_, o)| is_type(&o.dict, "Catalog")).and_then(|(_, o)| ref_of(&o.dict, "/Pages")) else {
        return vec![];
    };
    let mut out = Vec::new();
    let mut stack = vec![root];
    let mut seen = std::collections::HashSet::new();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(o) = objs.get(&id) else { continue };
        if is_type(&o.dict, "Pages") {
            let kids = refs_in_array(&o.dict, "/Kids");
            for k in kids.into_iter().rev() {
                stack.push(k);
            }
        } else if is_type(&o.dict, "Page") {
            out.push(id);
        }
    }
    out
}

/// `/Kids [3 0 R 4 0 R]` → [3, 4].
fn refs_in_array(dict: &str, key: &str) -> Vec<u32> {
    let Some(i) = dict.find(key) else { return vec![] };
    let rest = dict[i + key.len()..].trim_start();
    if !rest.starts_with('[') {
        return ref_of(dict, key).into_iter().collect();
    }
    let inner = &rest[1..rest.find(']').unwrap_or(rest.len())];
    let toks: Vec<&str> = inner.split_whitespace().collect();
    toks.windows(3)
        .filter(|w| w[2] == "R")
        .filter_map(|w| w[0].parse().ok())
        .collect()
}

fn contents_of(page: &str) -> Vec<u32> {
    refs_in_array(page, "/Contents")
}

/// A font's name on the page (`F1`) → how to turn its codes into letters.
#[derive(Debug, Default, Clone)]
struct Font {
    map: HashMap<u32, String>,
    /// Codes are two bytes (Identity-H and most CID fonts).
    two_byte: bool,
}

fn fonts_of(page: &str, objs: &HashMap<u32, Obj>, page_id: u32) -> HashMap<String, Font> {
    // /Resources inline, or a reference, or inherited from the parent.
    let mut res = resources_text(page, objs);
    if res.is_none() {
        let mut parent = ref_of(page, "/Parent");
        let mut hops = 0;
        while let (Some(p), true) = (parent, hops < 16) {
            let Some(o) = objs.get(&p) else { break };
            if let Some(r) = resources_text(&o.dict, objs) {
                res = Some(r);
                break;
            }
            parent = ref_of(&o.dict, "/Parent");
            hops += 1;
        }
    }
    let _ = page_id;
    let Some(res) = res else { return HashMap::new() };
    let font_dict = match ref_of(&res, "/Font") {
        Some(r) => objs.get(&r).map(|o| o.dict.clone()).unwrap_or_default(),
        None => inline_dict_after(&res, "/Font").unwrap_or_default(),
    };
    // `/F1 5 0 R /F2 6 0 R`
    let toks: Vec<&str> = font_dict.split_whitespace().collect();
    let mut out = HashMap::new();
    for w in toks.windows(4) {
        if let (Some(name), Ok(id), "R") = (w[0].strip_prefix('/'), w[1].parse::<u32>(), w[3]) {
            let name = name.trim_start_matches("<<").to_string();
            if let Some(f) = objs.get(&id) {
                out.insert(name, font_from(&f.dict, objs));
            }
        }
    }
    out
}

fn resources_text(dict: &str, objs: &HashMap<u32, Obj>) -> Option<String> {
    if let Some(r) = ref_of(dict, "/Resources") {
        return objs.get(&r).map(|o| o.dict.clone());
    }
    inline_dict_after(dict, "/Resources")
}

/// The `<< … >>` that follows a key, nesting counted.
fn inline_dict_after(dict: &str, key: &str) -> Option<String> {
    let i = dict.find(key)?;
    let rest = &dict[i + key.len()..];
    let open = rest.find("<<")?;
    if !rest[..open].trim().is_empty() {
        return None;
    }
    let b = rest.as_bytes();
    let (mut depth, mut j) = (0i32, open);
    while j + 1 < b.len() {
        if &b[j..j + 2] == b"<<" {
            depth += 1;
            j += 2;
        } else if &b[j..j + 2] == b">>" {
            depth -= 1;
            j += 2;
            if depth == 0 {
                return Some(rest[open + 2..j - 2].to_string());
            }
        } else {
            j += 1;
        }
    }
    None
}

fn font_from(dict: &str, objs: &HashMap<u32, Obj>) -> Font {
    let two_byte = dict.contains("/Type0") || dict.contains("Identity-H") || dict.contains("Identity-V");
    let mut font = Font { two_byte, ..Default::default() };
    if let Some(r) = ref_of(dict, "/ToUnicode") {
        if let Some(Obj { stream: Some(s), .. }) = objs.get(&r) {
            let (map, two) = cmap(&String::from_utf8_lossy(s));
            font.map = map;
            font.two_byte = font.two_byte || two;
        }
    }
    font
}

/// A ToUnicode CMap: `beginbfchar` pairs and `beginbfrange` runs.
fn cmap(text: &str) -> (HashMap<u32, String>, bool) {
    let mut map = HashMap::new();
    let mut two_byte = false;
    if let Some(i) = text.find("begincodespacerange") {
        let seg = &text[i..];
        if let Some(h) = seg.find('<') {
            let len = seg[h + 1..].find('>').unwrap_or(0);
            two_byte = len >= 4;
        }
    }
    for block in text.split("beginbfchar").skip(1) {
        let block = block.split("endbfchar").next().unwrap_or("");
        let hexes = hex_tokens(block);
        for pair in hexes.chunks(2) {
            if let [src, dst] = pair {
                map.insert(hex_num(src), utf16_hex(dst));
            }
        }
    }
    for block in text.split("beginbfrange").skip(1) {
        let block = block.split("endbfrange").next().unwrap_or("");
        for line in block.lines() {
            let hexes = hex_tokens(line);
            if hexes.len() < 3 {
                continue;
            }
            let (lo, hi) = (hex_num(&hexes[0]), hex_num(&hexes[1]));
            if hi < lo || hi - lo > 10_000 {
                continue;
            }
            if line.contains('[') {
                for (k, dst) in hexes[2..].iter().enumerate() {
                    map.insert(lo + k as u32, utf16_hex(dst));
                }
            } else {
                let start = utf16_hex(&hexes[2]);
                let mut chars: Vec<char> = start.chars().collect();
                for code in lo..=hi {
                    map.insert(code, chars.iter().collect());
                    if let Some(last) = chars.last_mut() {
                        *last = char::from_u32(*last as u32 + 1).unwrap_or(*last);
                    }
                }
            }
        }
    }
    (map, two_byte)
}

fn hex_tokens(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(a) = rest.find('<') {
        let Some(b) = rest[a..].find('>') else { break };
        out.push(rest[a + 1..a + b].chars().filter(|c| c.is_ascii_hexdigit()).collect());
        rest = &rest[a + b + 1..];
    }
    out
}

fn hex_num(h: &str) -> u32 {
    u32::from_str_radix(h, 16).unwrap_or(0)
}

fn hex_bytes(h: &str) -> Vec<u8> {
    let digits: Vec<u8> = h.bytes().filter(|c| c.is_ascii_hexdigit()).collect();
    digits
        .chunks(2)
        .map(|p| {
            let s = if p.len() == 2 { format!("{}{}", p[0] as char, p[1] as char) } else { format!("{}0", p[0] as char) };
            u8::from_str_radix(&s, 16).unwrap_or(0)
        })
        .collect()
}

fn utf16_hex(h: &str) -> String {
    let b = hex_bytes(h);
    let units: Vec<u16> = b.chunks(2).map(|p| if p.len() == 2 { u16::from_be_bytes([p[0], p[1]]) } else { p[0] as u16 }).collect();
    String::from_utf16_lossy(&units)
}

// ---------------------------------------------------------------- content

/// The strings a content stream draws, with line breaks where it moves down
/// a line and spaces where a `TJ` gap is wide enough to be one.
fn text_of(stream: &[u8], fonts: &HashMap<String, Font>) -> String {
    let mut out = String::new();
    let mut font: Option<&Font> = None;
    let mut operands: Vec<Tok> = Vec::new();
    let mut last_y: Option<f64> = None;
    for tok in tokens(stream) {
        match tok {
            Tok::Op(op) => {
                match op.as_str() {
                    "Tf" => {
                        if let Some(Tok::Name(n)) = operands.iter().rev().find(|t| matches!(t, Tok::Name(_))) {
                            font = fonts.get(n);
                        }
                    }
                    "Tj" | "'" | "\"" => {
                        if op != "Tj" {
                            newline(&mut out);
                        }
                        if let Some(Tok::Str(s)) = operands.last() {
                            out.push_str(&decode_str(s, font));
                        }
                    }
                    "TJ" => {
                        if let Some(Tok::Array(items)) = operands.last() {
                            for it in items {
                                match it {
                                    Tok::Str(s) => out.push_str(&decode_str(s, font)),
                                    Tok::Num(n) if *n < -200.0 && !out.ends_with(' ') => out.push(' '),
                                    _ => {}
                                }
                            }
                        }
                    }
                    "Td" | "TD" => {
                        let ty = match operands.as_slice() {
                            [.., Tok::Num(_), Tok::Num(y)] => *y,
                            _ => 0.0,
                        };
                        if ty.abs() > 0.1 {
                            newline(&mut out);
                        } else if !out.ends_with(' ') && !out.is_empty() {
                            out.push(' ');
                        }
                    }
                    "Tm" => {
                        if let [.., Tok::Num(_), Tok::Num(y)] = operands.as_slice() {
                            if last_y.is_some_and(|ly| (ly - y).abs() > 0.5) {
                                newline(&mut out);
                            } else if !out.ends_with(' ') && !out.is_empty() {
                                out.push(' ');
                            }
                            last_y = Some(*y);
                        }
                    }
                    "T*" => newline(&mut out),
                    "ET"
                        if !out.ends_with('\n') && !out.ends_with(' ') && !out.is_empty() => {
                            out.push(' ');
                        }
                    _ => {}
                }
                operands.clear();
            }
            other => operands.push(other),
        }
    }
    out
}

fn newline(out: &mut String) {
    while out.ends_with(' ') {
        out.pop();
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
}

fn decode_str(bytes: &[u8], font: Option<&Font>) -> String {
    match font {
        Some(f) if !f.map.is_empty() => {
            let mut s = String::new();
            if f.two_byte {
                for p in bytes.chunks(2) {
                    let code = if p.len() == 2 { u16::from_be_bytes([p[0], p[1]]) as u32 } else { p[0] as u32 };
                    s.push_str(f.map.get(&code).map(|x| x.as_str()).unwrap_or(""));
                }
            } else {
                for b in bytes {
                    match f.map.get(&(*b as u32)) {
                        Some(x) => s.push_str(x),
                        None => s.push(*b as char),
                    }
                }
            }
            s
        }
        Some(f) if f.two_byte => {
            // No map: two-byte codes are glyph numbers, not letters.
            String::new()
        }
        _ => bytes.iter().map(|b| *b as char).collect(),
    }
}

#[derive(Debug, Clone)]
enum Tok {
    Num(f64),
    Str(Vec<u8>),
    Name(String),
    Array(Vec<Tok>),
    Op(String),
}

fn tokens(s: &[u8]) -> Vec<Tok> {
    let mut i = 0;
    let mut out = Vec::new();
    let mut stack: Vec<Vec<Tok>> = Vec::new();
    let push = |t: Tok, out: &mut Vec<Tok>, stack: &mut Vec<Vec<Tok>>| match stack.last_mut() {
        Some(a) => a.push(t),
        None => out.push(t),
    };
    while i < s.len() {
        let c = s[i];
        match c {
            b' ' | b'\n' | b'\r' | b'\t' | 0x0c | 0 => i += 1,
            b'%' => {
                while i < s.len() && s[i] != b'\n' && s[i] != b'\r' {
                    i += 1;
                }
            }
            b'(' => {
                let (bytes, j) = literal(s, i + 1);
                push(Tok::Str(bytes), &mut out, &mut stack);
                i = j;
            }
            b'<' if s.get(i + 1) == Some(&b'<') => {
                // An inline dictionary (marked content); skip to its end.
                let mut depth = 0;
                while i + 1 < s.len() {
                    if &s[i..i + 2] == b"<<" {
                        depth += 1;
                        i += 2;
                    } else if &s[i..i + 2] == b">>" {
                        depth -= 1;
                        i += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
            }
            b'<' => {
                let end = s[i..].iter().position(|&b| b == b'>').map_or(s.len(), |p| i + p);
                push(Tok::Str(hex_bytes(&String::from_utf8_lossy(&s[i + 1..end]))), &mut out, &mut stack);
                i = end + 1;
            }
            b'[' => {
                stack.push(Vec::new());
                i += 1;
            }
            b']' => {
                let a = stack.pop().unwrap_or_default();
                push(Tok::Array(a), &mut out, &mut stack);
                i += 1;
            }
            b'/' => {
                let j = word_end(s, i + 1);
                push(Tok::Name(String::from_utf8_lossy(&s[i + 1..j]).into_owned()), &mut out, &mut stack);
                i = j;
            }
            b'0'..=b'9' | b'-' | b'+' | b'.' => {
                let j = word_end(s, i + 1);
                let n = String::from_utf8_lossy(&s[i..j]).parse().unwrap_or(0.0);
                push(Tok::Num(n), &mut out, &mut stack);
                i = j;
            }
            _ => {
                let j = word_end(s, i + 1).max(i + 1);
                let op = String::from_utf8_lossy(&s[i..j]).into_owned();
                if op == "BI" {
                    // An inline image: its data is binary, skip to `EI`.
                    match find(&s[j..], b"EI") {
                        Some(k) => i = j + k + 2,
                        None => break,
                    }
                    continue;
                }
                stack.clear();
                out.push(Tok::Op(op));
                i = j;
            }
        }
    }
    out
}

fn word_end(s: &[u8], mut j: usize) -> usize {
    while j < s.len() && !b" \n\r\t\x0c\0()<>[]{}/%".contains(&s[j]) {
        j += 1;
    }
    j
}

/// A `( … )` string, with its escapes and nested brackets.
fn literal(s: &[u8], mut i: usize) -> (Vec<u8>, usize) {
    let mut out = Vec::new();
    let mut depth = 1;
    while i < s.len() {
        let c = s[i];
        match c {
            b'\\' => {
                i += 1;
                let Some(&e) = s.get(i) else { break };
                match e {
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'b' => out.push(8),
                    b'f' => out.push(12),
                    b'0'..=b'7' => {
                        let mut v = 0u32;
                        let mut k = 0;
                        while k < 3 && i < s.len() && (b'0'..=b'7').contains(&s[i]) {
                            v = v * 8 + (s[i] - b'0') as u32;
                            i += 1;
                            k += 1;
                        }
                        out.push(v as u8);
                        continue;
                    }
                    b'\r' | b'\n' => {}
                    other => out.push(other),
                }
                i += 1;
            }
            b'(' => {
                depth += 1;
                out.push(c);
                i += 1;
            }
            b')' => {
                depth -= 1;
                i += 1;
                if depth == 0 {
                    break;
                }
                out.push(c);
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    (out, i)
}

/// Collapse runs of spaces and blank lines the drawing order leaves behind.
fn tidy(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let l = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if l.is_empty() && out.ends_with("\n\n") {
            continue;
        }
        out.push_str(&l);
        out.push('\n');
    }
    out.trim().to_string()
}
