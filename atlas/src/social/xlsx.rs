//! Reading a spreadsheet file (.xlsx): every sheet's name and its cells as
//! text. For LinkedIn, whose analytics come only as an .xlsx export.
//!
//! **Sources:** ECMA-376 Part 1 (Office Open XML SpreadsheetML): the package
//! is a zip; `xl/workbook.xml` names the sheets and points at each through
//! `xl/_rels/workbook.xml.rels`; `xl/sharedStrings.xml` holds the text
//! (`<si>`, plain `<t>` or rich-text runs `<r><t>`); each sheet's `<c>` cell
//! carries its reference (`r="B2"`), its type (`t="s"` a shared string,
//! `inlineStr`, `str`, `b`, or a number when absent) and its value `<v>`.
//! Dates are numbers: days since 1899-12-30 (the 1900 system).
//!
//! Why not a crate: the permissive ones (calamine, MIT) bring an XML parser,
//! a zip crate and an encoding crate for this one file; Atlas already reads
//! zips (`zipread`), and the part of SpreadsheetML an export uses is small.
//! Formulas, styles and merged cells are not read -- an export has values.

/// One sheet, as rows of cell text. Missing cells are empty strings, so a
/// column keeps its place.
#[derive(Debug, Clone, PartialEq)]
pub struct Sheet {
    pub name: String,
    pub rows: Vec<Vec<String>>,
}

/// The most one part may inflate to.
const MAX_PART: u64 = 64 * 1024 * 1024;
/// The most cells read from one sheet.
const MAX_CELLS: usize = 2_000_000;

/// Every sheet in the workbook, in the workbook's order.
pub fn sheets(zip: &[u8]) -> Result<Vec<Sheet>, String> {
    let part = |name: &str| -> Result<Option<String>, String> {
        Ok(crate::zipread::file_inside(zip, |n| n.eq_ignore_ascii_case(name), MAX_PART)?.map(|(_, b)| String::from_utf8_lossy(&b).into_owned()))
    };
    let workbook = part("xl/workbook.xml")?.ok_or("that isn't a spreadsheet (it has no workbook inside)")?;
    let rels = part("xl/_rels/workbook.xml.rels")?.unwrap_or_default();
    let shared = part("xl/sharedStrings.xml")?.map(|s| shared_strings(&s)).unwrap_or_default();

    let mut out = Vec::new();
    for tag in tags(&workbook, "sheet") {
        let name = attr(tag, "name").unwrap_or_default();
        let rid = attr(tag, "r:id").unwrap_or_default();
        let target = tags(&rels, "Relationship")
            .into_iter()
            .find(|r| attr(r, "Id").as_deref() == Some(rid.as_str()))
            .and_then(|r| attr(r, "Target"))
            .unwrap_or_default();
        if target.is_empty() {
            continue;
        }
        let path = if let Some(abs) = target.strip_prefix('/') { abs.to_string() } else { format!("xl/{target}") };
        let Some(xml) = part(&path)? else { continue };
        out.push(Sheet { name, rows: cells(&xml, &shared)? });
    }
    if out.is_empty() {
        return Err("that spreadsheet has no sheets I could read".into());
    }
    Ok(out)
}

/// Excel's day number to days since 1970 (the 1900 date system, which
/// every modern export uses).
pub fn excel_day(serial: f64) -> i64 {
    serial.floor() as i64 - 25_569
}

// ---------------------------------------------------------------- small XML

/// The opening tags named `name` (with or without a namespace prefix), each
/// as the text between `<` and `>`.
fn tags<'a>(xml: &'a str, name: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(at) = xml[i..].find('<') {
        let start = i + at + 1;
        let Some(end) = xml[start..].find('>') else { break };
        let tag = &xml[start..start + end];
        let tag_name = tag.split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
        if tag_name.rsplit(':').next() == Some(name) {
            out.push(tag);
        }
        i = start + end + 1;
    }
    out
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let mut from = 0;
    while let Some(i) = tag[from..].find(name) {
        let at = from + i;
        from = at + name.len();
        let before_ok = at > 0 && tag.as_bytes()[at - 1].is_ascii_whitespace();
        let rest = tag[at + name.len()..].trim_start();
        if !before_ok || !rest.starts_with('=') {
            continue;
        }
        let v = rest[1..].trim_start();
        let q = v.chars().next()?;
        if q != '"' && q != '\'' {
            return None;
        }
        let end = v[1..].find(q)?;
        return Some(unescape(&v[1..1 + end]));
    }
    None
}

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        // Cut back to a character boundary (the same panic `feeds` had).
        let mut lim = tail.len().min(12);
        while !tail.is_char_boundary(lim) {
            lim -= 1;
        }
        let semi = tail[..lim].find(';');
        let (ch, used) = match semi {
            Some(n) => {
                let ent = &tail[1..n];
                let c = match ent {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    e if e.starts_with("#x") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32),
                    e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
                    _ => None,
                };
                (c, n + 1)
            }
            None => (None, 1),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &tail[used..];
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

/// The text of every `<t>` inside `xml`, run together (rich text is split
/// into runs, each with its own `<t>`).
fn texts_in(xml: &str) -> String {
    let mut out = String::new();
    let mut i = 0;
    while let Some(at) = xml[i..].find("<t") {
        let start = i + at;
        let after = &xml[start + 2..];
        // `<t>` or `<t xml:space="preserve">`, not `<tab>` or `<totals>`.
        if !(after.starts_with('>') || after.starts_with(' ')) {
            i = start + 2;
            continue;
        }
        let Some(open_end) = after.find('>') else { break };
        if after[..open_end].ends_with('/') {
            i = start + 2 + open_end + 1;
            continue;
        }
        let body_start = start + 2 + open_end + 1;
        let Some(close) = xml[body_start..].find("</t>") else { break };
        out.push_str(&unescape(&xml[body_start..body_start + close]));
        i = body_start + close + 4;
    }
    out
}

fn shared_strings(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(at) = xml[i..].find("<si") {
        let start = i + at;
        let rest = &xml[start..];
        let end = match (rest.find("</si>"), rest.find("/>")) {
            (Some(c), _) => c + 5,
            (None, Some(s)) => s + 2,
            _ => break,
        };
        out.push(texts_in(&rest[..end]));
        i = start + end;
    }
    out
}

/// "B12" -> column 1 (zero-based).
fn column_of(r: &str) -> Option<usize> {
    let letters: String = r.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    if letters.is_empty() || letters.len() > 3 {
        return None;
    }
    Some(letters.to_ascii_uppercase().bytes().fold(0usize, |n, b| n * 26 + (b - b'A' + 1) as usize) - 1)
}

fn cells(xml: &str, shared: &[String]) -> Result<Vec<Vec<String>>, String> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut seen = 0usize;
    let mut i = 0;
    while let Some(at) = xml[i..].find("<row") {
        let row_start = i + at;
        let rest = &xml[row_start..];
        let row_open_end = rest.find('>').ok_or("a row never closes")?;
        let self_closing = rest[..row_open_end].ends_with('/');
        let row_end = if self_closing { row_open_end + 1 } else { rest.find("</row>").map(|n| n + 6).ok_or("a row never closes")? };
        let row_xml = &rest[..row_end];
        // Rows can skip numbers (an empty row isn't written); the row's own
        // `r` keeps them where they belong.
        let r_num = attr(&rest[1..row_open_end], "r").and_then(|n| n.parse::<usize>().ok());
        let mut row: Vec<String> = Vec::new();
        let mut j = 0;
        while let Some(c_at) = row_xml[j..].find("<c") {
            let c_start = j + c_at;
            let c_rest = &row_xml[c_start..];
            let next = c_rest.as_bytes().get(2).copied().unwrap_or(b'>');
            if !(next == b' ' || next == b'>' || next == b'/') {
                j = c_start + 2;
                continue;
            }
            let open_end = c_rest.find('>').ok_or("a cell never closes")?;
            let open = &c_rest[1..open_end];
            let (body, len) = if open.ends_with('/') {
                ("", open_end + 1)
            } else {
                let close = c_rest.find("</c>").ok_or("a cell never closes")?;
                (&c_rest[open_end + 1..close], close + 4)
            };
            let kind = attr(open, "t").unwrap_or_default();
            let v = body.find("<v>").and_then(|s| body[s + 3..].find("</v>").map(|e| unescape(&body[s + 3..s + 3 + e])));
            let text = match kind.as_str() {
                "s" => v.and_then(|n| n.trim().parse::<usize>().ok()).and_then(|n| shared.get(n).cloned()).unwrap_or_default(),
                "inlineStr" => texts_in(body),
                "b" => match v.as_deref() {
                    Some("1") => "TRUE".into(),
                    Some(_) => "FALSE".into(),
                    None => String::new(),
                },
                _ => v.unwrap_or_default(),
            };
            let col = attr(open, "r").and_then(|r| column_of(&r)).unwrap_or(row.len());
            if col > 16_384 {
                return Err("a cell is further right than a spreadsheet goes".into());
            }
            while row.len() < col {
                row.push(String::new());
            }
            if row.len() == col {
                row.push(text);
            } else {
                row[col] = text;
            }
            seen += 1;
            if seen > MAX_CELLS {
                return Err(format!("that sheet has more than {MAX_CELLS} cells, more than an export does"));
            }
            j = c_start + len;
        }
        if let Some(n) = r_num {
            while rows.len() + 1 < n {
                rows.push(Vec::new());
            }
        }
        rows.push(row);
        i = row_start + row_end;
    }
    Ok(rows)
}
