//! PDF pages, moved about in house: merge files, split one into pages, take
//! a range out, stamp a signature image onto a page -- without a PDF
//! library.
//!
//! **Sources:** ISO 32000-1:2008 (PDF 1.7) -- §7.3 objects, §7.5 file
//! structure (including §7.5.7 object streams), §7.4.4 FlateDecode and its
//! PNG predictors (§7.4.4.4, from RFC 2083), §7.7.3 the page tree and its
//! inheritable attributes, §8.9.5 image XObjects and soft masks. `lopdf`
//! (MIT) was read as a reference for the shape of a writer that renumbers
//! objects; nothing is copied. Inflate is the tree's own (`zipread`).
//!
//! **How it reads.** Not by trusting the cross-reference table: many PDFs
//! in the wild have one that's slightly wrong, and a reader that trusts it
//! fails on them. It walks the file once, object by object (`N G obj …
//! endobj`), skipping stream bytes by their length, so a later definition of
//! an object replaces an earlier one exactly as an incremental update means
//! it to; objects packed inside object streams are unpacked the same way.
//! Password-protected files are refused, never half-read.
//!
//! **How it writes.** Only the objects the chosen pages reach, renumbered
//! from 1, streams copied byte for byte (no re-encoding, so nothing is lost
//! and nothing is slow), a fresh page tree with the attributes each page
//! inherited written onto the page itself, and a plain cross-reference
//! table. Every file it writes is read back and its pages counted before
//! it's reported done (`check_written`). It never writes over the file it
//! read.

use std::collections::{BTreeMap, HashMap, HashSet};

/// The biggest file it will open.
pub const MAX_BYTES: usize = 256 * 1024 * 1024;
/// The most objects it will hold from one file.
pub const MAX_OBJECTS: usize = 2_000_000;

#[derive(Debug, Clone, PartialEq)]
pub enum Obj {
    Null,
    Bool(bool),
    Int(i64),
    Real(f64),
    Str(Vec<u8>),
    Name(String),
    Array(Vec<Obj>),
    Dict(Dict),
    Ref(u32, u16),
    Stream(Dict, Vec<u8>),
}

pub type Dict = Vec<(String, Obj)>;

pub fn dict_get<'a>(d: &'a Dict, key: &str) -> Option<&'a Obj> {
    d.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

fn set(d: &mut Dict, key: &str, v: Obj) {
    match d.iter_mut().find(|(k, _)| k == key) {
        Some(slot) => slot.1 = v,
        None => d.push((key.to_string(), v)),
    }
}

fn remove(d: &mut Dict, key: &str) {
    d.retain(|(k, _)| k != key);
}

// ---------------------------------------------------------------- lexing

struct Lex<'a> {
    b: &'a [u8],
    i: usize,
}

fn is_white(c: u8) -> bool {
    matches!(c, 0 | 9 | 10 | 12 | 13 | 32)
}
fn is_delim(c: u8) -> bool {
    matches!(c, b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%')
}

impl<'a> Lex<'a> {
    fn skip_space(&mut self) {
        while self.i < self.b.len() {
            let c = self.b[self.i];
            if is_white(c) {
                self.i += 1;
            } else if c == b'%' {
                while self.i < self.b.len() && self.b[self.i] != b'\n' && self.b[self.i] != b'\r' {
                    self.i += 1;
                }
            } else {
                break;
            }
        }
    }

    fn word(&mut self) -> &'a [u8] {
        let s = self.i;
        while self.i < self.b.len() && !is_white(self.b[self.i]) && !is_delim(self.b[self.i]) {
            self.i += 1;
        }
        &self.b[s..self.i]
    }

    fn peek_keyword(&self, k: &[u8]) -> bool {
        self.b[self.i..].starts_with(k)
    }

    /// One object at the cursor. `depth` bounds nesting, so a hostile file
    /// can't recurse the stack away.
    fn object(&mut self, depth: u32) -> Result<Obj, String> {
        if depth > 64 {
            return Err("nested too deeply".into());
        }
        self.skip_space();
        let c = *self.b.get(self.i).ok_or("ran out of file")?;
        match c {
            b'/' => {
                self.i += 1;
                let raw = self.word();
                Ok(Obj::Name(unescape_name(raw)))
            }
            b'(' => self.literal().map(Obj::Str),
            b'<' if self.b.get(self.i + 1) == Some(&b'<') => {
                self.i += 2;
                let mut d = Dict::new();
                loop {
                    self.skip_space();
                    if self.peek_keyword(b">>") {
                        self.i += 2;
                        break;
                    }
                    match self.object(depth + 1)? {
                        Obj::Name(k) => {
                            let v = self.object(depth + 1)?;
                            d.push((k, v));
                        }
                        _ => return Err("a dictionary key that isn't a name".into()),
                    }
                }
                Ok(Obj::Dict(d))
            }
            b'<' => {
                self.i += 1;
                let mut hex = Vec::new();
                while self.i < self.b.len() && self.b[self.i] != b'>' {
                    if self.b[self.i].is_ascii_hexdigit() {
                        hex.push(self.b[self.i]);
                    }
                    self.i += 1;
                }
                self.i += 1;
                if hex.len() % 2 == 1 {
                    hex.push(b'0');
                }
                Ok(Obj::Str(hex.chunks(2).map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap_or("00"), 16).unwrap_or(0)).collect()))
            }
            b'[' => {
                self.i += 1;
                let mut a = Vec::new();
                loop {
                    self.skip_space();
                    if self.b.get(self.i) == Some(&b']') {
                        self.i += 1;
                        break;
                    }
                    if self.i >= self.b.len() {
                        return Err("an array that never closes".into());
                    }
                    a.push(self.object(depth + 1)?);
                }
                Ok(Obj::Array(fold_refs(a)))
            }
            _ => {
                let w = self.word();
                if w.is_empty() {
                    self.i += 1;
                    return Err(format!("unexpected byte {c:#x}"));
                }
                match w {
                    b"null" => Ok(Obj::Null),
                    b"true" => Ok(Obj::Bool(true)),
                    b"false" => Ok(Obj::Bool(false)),
                    _ => {
                        let s = std::str::from_utf8(w).map_err(|_| "a bad number")?;
                        if let Ok(n) = s.parse::<i64>() {
                            // "12 0 R" is a reference: look ahead.
                            let save = self.i;
                            self.skip_space();
                            let g = self.word();
                            if let Ok(gen) = std::str::from_utf8(g).unwrap_or("").parse::<u16>() {
                                self.skip_space();
                                if self.b.get(self.i) == Some(&b'R') && self.b.get(self.i + 1).map(|c| is_white(*c) || is_delim(*c)).unwrap_or(true) {
                                    self.i += 1;
                                    return Ok(Obj::Ref(n.clamp(0, u32::MAX as i64) as u32, gen));
                                }
                            }
                            self.i = save;
                            Ok(Obj::Int(n))
                        } else if let Ok(r) = s.parse::<f64>() {
                            Ok(Obj::Real(r))
                        } else {
                            Err(format!("an unknown word \"{}\"", String::from_utf8_lossy(w)))
                        }
                    }
                }
            }
        }
    }

    fn literal(&mut self) -> Result<Vec<u8>, String> {
        self.i += 1;
        let mut out = Vec::new();
        let mut depth = 1;
        while self.i < self.b.len() {
            let c = self.b[self.i];
            self.i += 1;
            match c {
                b'\\' => {
                    let e = *self.b.get(self.i).ok_or("a string cut short")?;
                    self.i += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'\r' => {
                            if self.b.get(self.i) == Some(&b'\n') {
                                self.i += 1;
                            }
                        }
                        b'\n' => {}
                        b'0'..=b'7' => {
                            let mut v = (e - b'0') as u32;
                            for _ in 0..2 {
                                match self.b.get(self.i) {
                                    Some(d @ b'0'..=b'7') => {
                                        v = v * 8 + (d - b'0') as u32;
                                        self.i += 1;
                                    }
                                    _ => break,
                                }
                            }
                            out.push((v & 0xFF) as u8);
                        }
                        other => out.push(other),
                    }
                }
                b'(' => {
                    depth += 1;
                    out.push(c);
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(out);
                    }
                    out.push(c);
                }
                _ => out.push(c),
            }
        }
        Err("a string that never closes".into())
    }
}

/// Arrays are lexed flat; "12 0 R" inside one arrives as Int Int Name("R")
/// only if the look-ahead missed, which it doesn't -- kept as a no-op hook.
fn fold_refs(a: Vec<Obj>) -> Vec<Obj> {
    a
}

fn unescape_name(raw: &[u8]) -> String {
    let mut out = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'#' && i + 2 < raw.len() {
            if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&raw[i + 1..i + 3]).unwrap_or("zz"), 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(raw[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

// ---------------------------------------------------------------- the file

pub struct Doc {
    pub objects: HashMap<u32, Obj>,
    pub root: u32,
    /// Page object numbers, in reading order.
    pub pages: Vec<u32>,
}

fn find_from(b: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    if from >= b.len() {
        return None;
    }
    b[from..].windows(pat.len()).position(|w| w == pat).map(|p| p + from)
}

/// Decode a stream's bytes: FlateDecode (with PNG or TIFF-2 predictors) or
/// none. Anything else is refused -- it's only needed for object streams.
fn decoded(d: &Dict, raw: &[u8]) -> Result<Vec<u8>, String> {
    let filters: Vec<String> = match dict_get(d, "Filter") {
        None => Vec::new(),
        Some(Obj::Name(n)) => vec![n.clone()],
        Some(Obj::Array(a)) => a.iter().filter_map(|o| if let Obj::Name(n) = o { Some(n.clone()) } else { None }).collect(),
        _ => return Err("a stream filter I can't read".into()),
    };
    let mut data = raw.to_vec();
    for f in &filters {
        match f.as_str() {
            "FlateDecode" | "Fl" => {
                if data.len() < 2 {
                    return Err("an empty compressed stream".into());
                }
                // zlib: two header bytes, then DEFLATE.
                data = crate::zipread::inflate(&data[2..], 512 * 1024 * 1024)?;
            }
            other => return Err(format!("a stream packed with {other}, which I don't unpack")),
        }
    }
    if let Some(Obj::Dict(p)) = dict_get(d, "DecodeParms") {
        let predictor = match dict_get(p, "Predictor") { Some(Obj::Int(n)) => *n, _ => 1 };
        let columns = match dict_get(p, "Columns") { Some(Obj::Int(n)) => (*n).max(1) as usize, _ => 1 };
        if predictor >= 10 {
            data = unpredict_png(&data, columns)?;
        } else if predictor != 1 {
            return Err(format!("predictor {predictor}, which I don't undo"));
        }
    }
    Ok(data)
}

fn unpredict_png(data: &[u8], columns: usize) -> Result<Vec<u8>, String> {
    let row = columns + 1;
    let mut out = Vec::with_capacity(data.len());
    let mut prev = vec![0u8; columns];
    for chunk in data.chunks(row) {
        if chunk.len() < row {
            break;
        }
        let kind = chunk[0];
        let mut cur = chunk[1..].to_vec();
        for i in 0..columns {
            let left = if i > 0 { cur[i - 1] } else { 0 };
            let up = prev[i];
            let ul = if i > 0 { prev[i - 1] } else { 0 };
            cur[i] = match kind {
                0 => cur[i],
                1 => cur[i].wrapping_add(left),
                2 => cur[i].wrapping_add(up),
                3 => cur[i].wrapping_add(((left as u16 + up as u16) / 2) as u8),
                4 => {
                    let p = left as i16 + up as i16 - ul as i16;
                    let (pa, pb, pc) = ((p - left as i16).abs(), (p - up as i16).abs(), (p - ul as i16).abs());
                    let pr = if pa <= pb && pa <= pc { left } else if pb <= pc { up } else { ul };
                    cur[i].wrapping_add(pr)
                }
                _ => return Err("a bad PNG predictor row".into()),
            };
        }
        out.extend_from_slice(&cur);
        prev = cur;
    }
    Ok(out)
}

impl Doc {
    /// Read a whole file. See the module header for why it walks rather
    /// than trusts the cross-reference table.
    pub fn parse(b: &[u8]) -> Result<Doc, String> {
        if b.len() > MAX_BYTES {
            return Err(format!("it's over {} MB, which is more than I'll open", MAX_BYTES / (1024 * 1024)));
        }
        if !b.starts_with(b"%PDF-") && find_from(b, 0, b"%PDF-").map(|p| p > 1024).unwrap_or(true) {
            return Err("it isn't a PDF (no %PDF- header)".into());
        }
        let mut objects: HashMap<u32, Obj> = HashMap::new();
        let mut roots: Vec<(u32, bool)> = Vec::new(); // (root, encrypted)
        let mut i = 0usize;
        while let Some(at) = find_from(b, i, b"obj") {
            i = at + 3;
            // "N G obj": walk back over the two numbers.
            let Some((num, _gen)) = header_before(b, at) else { continue };
            // A word that merely ends in "obj" ("endobj", "myobj") isn't one.
            if b.get(at + 3).map(|c| !is_white(*c) && !is_delim(*c)).unwrap_or(false) {
                continue;
            }
            let mut lx = Lex { b, i: at + 3 };
            let Ok(obj) = lx.object(0) else { continue };
            lx.skip_space();
            let obj = if lx.peek_keyword(b"stream") {
                lx.i += 6;
                if b.get(lx.i) == Some(&b'\r') {
                    lx.i += 1;
                }
                if b.get(lx.i) == Some(&b'\n') {
                    lx.i += 1;
                }
                let start = lx.i;
                let Obj::Dict(d) = obj else { continue };
                let direct_len = match dict_get(&d, "Length") {
                    Some(Obj::Int(n)) if *n >= 0 && start + (*n as usize) <= b.len() => {
                        let end = start + *n as usize;
                        // Trust the length only if "endstream" follows it.
                        let mut k = Lex { b, i: end };
                        k.skip_space();
                        if k.peek_keyword(b"endstream") { Some(end) } else { None }
                    }
                    _ => None,
                };
                let end = match direct_len {
                    Some(e) => e,
                    None => match find_from(b, start, b"endstream") {
                        Some(e) => {
                            // Drop the end-of-line before "endstream".
                            let mut e2 = e;
                            if e2 > start && b[e2 - 1] == b'\n' {
                                e2 -= 1;
                            }
                            if e2 > start && b[e2 - 1] == b'\r' {
                                e2 -= 1;
                            }
                            e2
                        }
                        None => continue,
                    },
                };
                i = end;
                Obj::Stream(d, b[start..end].to_vec())
            } else {
                i = lx.i;
                obj
            };
            if let Obj::Stream(d, _) | Obj::Dict(d) = &obj {
                if matches!(dict_get(d, "Type"), Some(Obj::Name(t)) if t == "XRef") {
                    if let Some(Obj::Ref(r, _)) = dict_get(d, "Root") {
                        roots.push((*r, dict_get(d, "Encrypt").is_some()));
                    }
                }
            }
            objects.insert(num, obj);
            if objects.len() > MAX_OBJECTS {
                return Err("it holds more objects than I'll read".into());
            }
        }
        // Classic trailers.
        let mut t = 0;
        while let Some(at) = find_from(b, t, b"trailer") {
            t = at + 7;
            let mut lx = Lex { b, i: t };
            if let Ok(Obj::Dict(d)) = lx.object(0) {
                if let Some(Obj::Ref(r, _)) = dict_get(&d, "Root") {
                    roots.push((*r, dict_get(&d, "Encrypt").is_some()));
                }
            }
        }
        if roots.iter().any(|(_, enc)| *enc) {
            return Err("it's password-protected (encrypted); I don't open those".into());
        }
        // Unpack object streams: their objects count unless the file defines
        // the same number directly later (a direct definition is newer).
        let streams: Vec<(u32, Dict, Vec<u8>)> = objects
            .iter()
            .filter_map(|(n, o)| match o {
                Obj::Stream(d, raw) if matches!(dict_get(d, "Type"), Some(Obj::Name(t)) if t == "ObjStm") => Some((*n, d.clone(), raw.clone())),
                _ => None,
            })
            .collect();
        for (_, d, raw) in streams {
            let Ok(data) = decoded(&d, &raw) else { continue };
            let n = match dict_get(&d, "N") { Some(Obj::Int(n)) => *n as usize, _ => continue };
            let first = match dict_get(&d, "First") { Some(Obj::Int(n)) => *n as usize, _ => continue };
            let mut lx = Lex { b: &data, i: 0 };
            let mut pairs = Vec::new();
            for _ in 0..n {
                match (lx.object(0), lx.object(0)) {
                    (Ok(Obj::Int(num)), Ok(Obj::Int(off))) => pairs.push((num as u32, off as usize)),
                    _ => break,
                }
            }
            for (num, off) in pairs {
                if objects.contains_key(&num) {
                    continue;
                }
                let mut ol = Lex { b: &data, i: first + off };
                if let Ok(o) = ol.object(0) {
                    objects.insert(num, o);
                }
            }
        }
        let root = roots
            .iter()
            .rev()
            .map(|(r, _)| *r)
            .find(|r| matches!(objects.get(r), Some(Obj::Dict(d)) if dict_get(d, "Pages").is_some()))
            .or_else(|| {
                // No trailer found: the catalogue is the object that says so.
                objects.iter().find(|(_, o)| matches!(o, Obj::Dict(d) if matches!(dict_get(d, "Type"), Some(Obj::Name(t)) if t == "Catalog"))).map(|(n, _)| *n)
            })
            .ok_or("I couldn't find its catalogue (the file is damaged)")?;
        let mut doc = Doc { objects, root, pages: Vec::new() };
        doc.pages = doc.page_list()?;
        if doc.pages.is_empty() {
            return Err("it has no pages".into());
        }
        Ok(doc)
    }

    pub fn resolve<'a>(&'a self, o: &'a Obj) -> &'a Obj {
        let mut cur = o;
        for _ in 0..16 {
            match cur {
                Obj::Ref(n, _) => match self.objects.get(n) {
                    Some(x) => cur = x,
                    None => return &Obj::Null,
                },
                _ => return cur,
            }
        }
        &Obj::Null
    }

    fn dict_of(&self, n: u32) -> Option<&Dict> {
        match self.objects.get(&n)? {
            Obj::Dict(d) => Some(d),
            _ => None,
        }
    }

    fn page_list(&self) -> Result<Vec<u32>, String> {
        let cat = self.dict_of(self.root).ok_or("its catalogue isn't a dictionary")?;
        let top = match dict_get(cat, "Pages") { Some(Obj::Ref(n, _)) => *n, _ => return Err("its catalogue names no pages".into()) };
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        self.walk(top, &mut out, &mut seen, 0);
        Ok(out)
    }

    fn walk(&self, n: u32, out: &mut Vec<u32>, seen: &mut HashSet<u32>, depth: u32) {
        if depth > 64 || !seen.insert(n) {
            return;
        }
        let Some(d) = self.dict_of(n) else { return };
        match dict_get(d, "Type") {
            Some(Obj::Name(t)) if t == "Page" => out.push(n),
            _ => {
                if let Some(Obj::Array(kids)) = dict_get(d, "Kids").map(|k| self.resolve(k)) {
                    for k in kids {
                        if let Obj::Ref(kn, _) = k {
                            self.walk(*kn, out, seen, depth + 1);
                        }
                    }
                }
            }
        }
    }

    /// A page's dictionary with what it inherits from its ancestors written
    /// onto it (§7.7.3.4: Resources, MediaBox, CropBox, Rotate).
    fn page_dict(&self, n: u32) -> Dict {
        let mut d = self.dict_of(n).cloned().unwrap_or_default();
        let mut parent = dict_get(&d, "Parent").cloned();
        for _ in 0..64 {
            let Some(Obj::Ref(p, _)) = parent else { break };
            let Some(pd) = self.dict_of(p) else { break };
            for key in ["Resources", "MediaBox", "CropBox", "Rotate"] {
                if dict_get(&d, key).is_none() {
                    if let Some(v) = dict_get(pd, key) {
                        d.push((key.to_string(), v.clone()));
                    }
                }
            }
            parent = dict_get(pd, "Parent").cloned();
        }
        remove(&mut d, "Parent");
        if dict_get(&d, "MediaBox").is_none() {
            // US Letter, the spec's own default when nothing says.
            d.push(("MediaBox".into(), Obj::Array(vec![Obj::Int(0), Obj::Int(0), Obj::Int(612), Obj::Int(792)])));
        }
        d
    }
}

/// "12 0" before "obj" at `at`: the object number, if it's really there.
fn header_before(b: &[u8], at: usize) -> Option<(u32, u16)> {
    let mut j = at;
    let skip_ws_back = |j: &mut usize| {
        while *j > 0 && is_white(b[*j - 1]) {
            *j -= 1;
        }
    };
    skip_ws_back(&mut j);
    let g_end = j;
    while j > 0 && b[j - 1].is_ascii_digit() {
        j -= 1;
    }
    let gen: u16 = std::str::from_utf8(&b[j..g_end]).ok()?.parse().ok()?;
    if g_end == j {
        return None;
    }
    skip_ws_back(&mut j);
    let n_end = j;
    while j > 0 && b[j - 1].is_ascii_digit() {
        j -= 1;
    }
    if n_end == j {
        return None;
    }
    // Preceded by the start of a line or whitespace, not by part of a word.
    if j > 0 && !is_white(b[j - 1]) && !is_delim(b[j - 1]) {
        return None;
    }
    let num: u32 = std::str::from_utf8(&b[j..n_end]).ok()?.parse().ok()?;
    Some((num, gen))
}

// ---------------------------------------------------------------- writing

fn write_obj(o: &Obj, out: &mut Vec<u8>) {
    match o {
        Obj::Null => out.extend_from_slice(b"null"),
        Obj::Bool(v) => out.extend_from_slice(if *v { b"true" } else { b"false" }),
        Obj::Int(n) => out.extend_from_slice(n.to_string().as_bytes()),
        Obj::Real(r) => {
            let s = if r.is_finite() { format!("{r:.6}") } else { "0".into() };
            let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
            out.extend_from_slice(if s.is_empty() || s == "-" { b"0" } else { s.as_bytes() });
        }
        Obj::Str(s) => {
            out.push(b'<');
            for c in s {
                out.extend_from_slice(format!("{c:02X}").as_bytes());
            }
            out.push(b'>');
        }
        Obj::Name(n) => {
            out.push(b'/');
            for c in n.bytes() {
                if c.is_ascii_alphanumeric() || b"-_.+*".contains(&c) {
                    out.push(c);
                } else {
                    out.extend_from_slice(format!("#{c:02X}").as_bytes());
                }
            }
        }
        Obj::Array(a) => {
            out.push(b'[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(b' ');
                }
                write_obj(x, out);
            }
            out.push(b']');
        }
        Obj::Dict(d) => write_dict(d, out),
        Obj::Ref(n, g) => out.extend_from_slice(format!("{n} {g} R").as_bytes()),
        Obj::Stream(d, data) => {
            let mut d = d.clone();
            set(&mut d, "Length", Obj::Int(data.len() as i64));
            write_dict(&d, out);
            out.extend_from_slice(b"\nstream\n");
            out.extend_from_slice(data);
            out.extend_from_slice(b"\nendstream");
        }
    }
}

fn write_dict(d: &Dict, out: &mut Vec<u8>) {
    out.extend_from_slice(b"<<");
    for (k, v) in d {
        write_obj(&Obj::Name(k.clone()), out);
        out.push(b' ');
        write_obj(v, out);
        out.push(b'\n');
    }
    out.extend_from_slice(b">>");
}

/// Collects the objects the chosen pages reach, renumbering as it goes.
struct Builder<'a> {
    docs: &'a [&'a Doc],
    /// (doc, old number) -> new number.
    map: HashMap<(usize, u32), u32>,
    objects: BTreeMap<u32, Obj>,
    next: u32,
}

impl<'a> Builder<'a> {
    fn alloc(&mut self) -> u32 {
        self.next += 1;
        self.next
    }

    /// Copy `o` from document `di`, translating references. A reference to
    /// a page that isn't being copied (a link's destination) becomes null,
    /// so taking page 3 doesn't drag the whole document along.
    fn translate(&mut self, di: usize, o: &Obj, chosen: &HashSet<u32>, depth: u32) -> Obj {
        if depth > 256 {
            return Obj::Null;
        }
        match o {
            Obj::Ref(n, _) => {
                let doc = self.docs[di];
                if doc.pages.contains(n) && !chosen.contains(n) {
                    return Obj::Null;
                }
                if let Some(new) = self.map.get(&(di, *n)) {
                    return Obj::Ref(*new, 0);
                }
                let Some(target) = doc.objects.get(n) else { return Obj::Null };
                let new = self.alloc();
                self.map.insert((di, *n), new);
                let target = target.clone();
                let t = self.translate(di, &target, chosen, depth + 1);
                self.objects.insert(new, t);
                Obj::Ref(new, 0)
            }
            Obj::Array(a) => Obj::Array(a.iter().map(|x| self.translate(di, x, chosen, depth + 1)).collect()),
            Obj::Dict(d) => Obj::Dict(d.iter().filter(|(k, _)| k != "Parent").map(|(k, v)| (k.clone(), self.translate(di, v, chosen, depth + 1))).collect()),
            Obj::Stream(d, data) => Obj::Stream(
                d.iter().filter(|(k, _)| k != "Length").map(|(k, v)| (k.clone(), self.translate(di, v, chosen, depth + 1))).collect(),
                data.clone(),
            ),
            other => other.clone(),
        }
    }
}

/// A page to put in a new file: from which document, which page (0-based).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRef {
    pub doc: usize,
    pub page: usize,
}

/// Something extra drawn on a page: an image XObject and the content that
/// places it.
pub struct Overlay {
    pub doc: usize,
    pub page: usize,
    pub image: Obj,
    pub mask: Option<Obj>,
    /// The content stream: "q w 0 0 h x y cm /AtlasStamp Do Q".
    pub content: Vec<u8>,
}

/// Write a new PDF holding `pages`, in that order, from `docs`.
pub fn write(docs: &[&Doc], pages: &[PageRef], overlay: Option<Overlay>) -> Result<Vec<u8>, String> {
    if pages.is_empty() {
        return Err("no pages to write".into());
    }
    let mut chosen: Vec<HashSet<u32>> = vec![HashSet::new(); docs.len()];
    for p in pages {
        let d = docs.get(p.doc).ok_or("a page from a file that isn't open")?;
        let n = *d.pages.get(p.page).ok_or_else(|| format!("there's no page {} (it has {})", p.page + 1, d.pages.len()))?;
        chosen[p.doc].insert(n);
    }
    let mut b = Builder { docs, map: HashMap::new(), objects: BTreeMap::new(), next: 0 };
    let catalog = b.alloc();
    let tree = b.alloc();
    let mut kids = Vec::new();
    for p in pages {
        let d = docs[p.doc];
        let old = d.pages[p.page];
        let mut dict = d.page_dict(old);
        // The same page twice in one file (a duplicate in a merge) must be
        // two page objects: pages are copied fresh each time.
        let key = (p.doc, old);
        let already = b.map.remove(&key);
        let new = b.alloc();
        b.map.insert(key, new);
        if let Some(ov) = overlay.as_ref().filter(|o| o.doc == p.doc && o.page == p.page) {
            add_overlay(d, &mut dict, ov, &mut b);
        }
        let translated = match b.translate(p.doc, &Obj::Dict(dict), &chosen[p.doc], 0) {
            Obj::Dict(mut x) => {
                set(&mut x, "Parent", Obj::Ref(tree, 0));
                x
            }
            _ => unreachable!(),
        };
        b.objects.insert(new, Obj::Dict(translated));
        kids.push(Obj::Ref(new, 0));
        if let Some(a) = already {
            b.map.insert(key, a);
        }
    }
    b.objects.insert(
        tree,
        Obj::Dict(vec![
            ("Type".into(), Obj::Name("Pages".into())),
            ("Count".into(), Obj::Int(kids.len() as i64)),
            ("Kids".into(), Obj::Array(kids)),
        ]),
    );
    b.objects.insert(catalog, Obj::Dict(vec![("Type".into(), Obj::Name("Catalog".into())), ("Pages".into(), Obj::Ref(tree, 0))]));
    Ok(serialise(&b.objects, catalog))
}

fn add_overlay(d: &Doc, page: &mut Dict, ov: &Overlay, b: &mut Builder) {
    // The image (and its mask) as new objects of the output.
    let mut image = ov.image.clone();
    if let Some(mask) = &ov.mask {
        let m = b.alloc();
        b.objects.insert(m, mask.clone());
        if let Obj::Stream(idict, _) = &mut image {
            set(idict, "SMask", Obj::Ref(m, 0));
        }
    }
    let img = b.alloc();
    b.objects.insert(img, image);
    let content = b.alloc();
    b.objects.insert(content, Obj::Stream(Dict::new(), ov.content.clone()));
    // Resources: the page's own (inherited ones are already on it), made
    // direct so a shared dictionary isn't changed for other pages. The
    // image is referred to by its NEW number, which `translate` would
    // otherwise try to look up in the source file -- so it is marked with a
    // name that can't clash and patched after translation via a direct Ref
    // to an object that exists only in the output.
    let mut res = match dict_get(page, "Resources").map(|r| d.resolve(r)) {
        Some(Obj::Dict(r)) => r.clone(),
        _ => Dict::new(),
    };
    let mut xo = match dict_get(&res, "XObject").map(|r| d.resolve(r)) {
        Some(Obj::Dict(x)) => x.clone(),
        _ => Dict::new(),
    };
    set(&mut xo, "AtlasStamp", Obj::Name(format!("\u{0}new{img}")));
    set(&mut res, "XObject", Obj::Dict(xo));
    set(page, "Resources", Obj::Dict(res));
    // Contents: keep the page's own, then draw the stamp on top.
    let mut contents = match dict_get(page, "Contents") {
        Some(Obj::Array(a)) => a.clone(),
        Some(o) => vec![o.clone()],
        None => Vec::new(),
    };
    contents.push(Obj::Name(format!("\u{0}new{content}")));
    set(page, "Contents", Obj::Array(contents));
}

fn patch_new_refs(o: &mut Obj) {
    match o {
        Obj::Name(n) if n.starts_with("\u{0}new") => {
            let num: u32 = n[4..].parse().unwrap_or(0);
            *o = Obj::Ref(num, 0);
        }
        Obj::Array(a) => a.iter_mut().for_each(patch_new_refs),
        Obj::Dict(d) | Obj::Stream(d, _) => d.iter_mut().for_each(|(_, v)| patch_new_refs(v)),
        _ => {}
    }
}

fn serialise(objects: &BTreeMap<u32, Obj>, root: u32) -> Vec<u8> {
    let mut out: Vec<u8> = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let size = objects.keys().max().copied().unwrap_or(0) + 1;
    let mut offsets = vec![0usize; size as usize];
    for (n, o) in objects {
        let mut o = o.clone();
        patch_new_refs(&mut o);
        offsets[*n as usize] = out.len();
        out.extend_from_slice(format!("{n} 0 obj\n").as_bytes());
        write_obj(&o, &mut out);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f\r\n").as_bytes());
    for n in 1..size {
        match objects.get(&n) {
            Some(_) => out.extend_from_slice(format!("{:010} 00000 n\r\n", offsets[n as usize]).as_bytes()),
            None => out.extend_from_slice(b"0000000000 65535 f\r\n"),
        }
    }
    out.extend_from_slice(format!("trailer\n<</Size {size} /Root {root} 0 R>>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
    out
}

/// Read back what was written and count its pages: nothing is reported
/// done until this agrees.
pub fn check_written(bytes: &[u8], expect_pages: usize) -> Result<(), String> {
    let d = Doc::parse(bytes).map_err(|e| format!("what I wrote doesn't read back: {e}"))?;
    if d.pages.len() != expect_pages {
        return Err(format!("what I wrote has {} pages, not {expect_pages}", d.pages.len()));
    }
    Ok(())
}

// ---------------------------------------------------------------- stamping

/// zlib with stored (uncompressed) DEFLATE blocks: valid FlateDecode data
/// with no compressor needed.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut chunks = data.chunks(65_535).peekable();
    if chunks.peek().is_none() {
        out.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
    }
    while let Some(c) = chunks.next() {
        out.push(if chunks.peek().is_none() { 1 } else { 0 });
        let len = c.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(c);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for x in data {
        a = (a + *x as u32) % 65_521;
        b = (b + a) % 65_521;
    }
    out.extend_from_slice(&((b << 16) | a).to_be_bytes());
    out
}

/// An image (RGBA, 4 bytes a pixel) placed on page `page` of document
/// `doc`: `width_pt` wide, its bottom-left `x_pt`, `y_pt` points from the
/// page's bottom-left corner.
pub fn stamp(doc: usize, page: usize, rgba: &[u8], w: u32, h: u32, x_pt: f64, y_pt: f64, width_pt: f64) -> Result<Overlay, String> {
    if w == 0 || h == 0 || rgba.len() != (w * h * 4) as usize {
        return Err("the image's size and its pixels don't agree".into());
    }
    let rgb: Vec<u8> = rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    let alpha: Vec<u8> = rgba.chunks_exact(4).map(|p| p[3]).collect();
    let img_dict = |cs: &str, data_len: usize| -> Dict {
        let _ = data_len;
        vec![
            ("Type".into(), Obj::Name("XObject".into())),
            ("Subtype".into(), Obj::Name("Image".into())),
            ("Width".into(), Obj::Int(w as i64)),
            ("Height".into(), Obj::Int(h as i64)),
            ("ColorSpace".into(), Obj::Name(cs.into())),
            ("BitsPerComponent".into(), Obj::Int(8)),
            ("Filter".into(), Obj::Name("FlateDecode".into())),
        ]
    };
    let image = Obj::Stream(img_dict("DeviceRGB", rgb.len()), zlib_stored(&rgb));
    let mask = alpha.iter().any(|a| *a != 255).then(|| Obj::Stream(img_dict("DeviceGray", alpha.len()), zlib_stored(&alpha)));
    let height_pt = width_pt * h as f64 / w as f64;
    let content = format!("q {width_pt:.2} 0 0 {height_pt:.2} {x_pt:.2} {y_pt:.2} cm /AtlasStamp Do Q\n").into_bytes();
    Ok(Overlay { doc, page, image, mask, content })
}

/// A page's size in points (from its MediaBox), for placing a stamp.
pub fn page_size(d: &Doc, page: usize) -> Option<(f64, f64)> {
    let n = *d.pages.get(page)?;
    let dict = d.page_dict(n);
    let num = |o: &Obj| match d.resolve(o) {
        Obj::Int(i) => Some(*i as f64),
        Obj::Real(r) => Some(*r),
        _ => None,
    };
    match dict_get(&dict, "MediaBox").map(|m| d.resolve(m)) {
        Some(Obj::Array(a)) if a.len() == 4 => Some((num(&a[2])? - num(&a[0])?, num(&a[3])? - num(&a[1])?)),
        _ => None,
    }
}

// ---------------------------------------------------------------- page ranges

/// "3", "2-5", "1,3,7-9", "all", "last": 0-based page indexes, in order.
pub fn ranges(spec: &str, count: usize) -> Result<Vec<usize>, String> {
    let s = spec.trim().to_lowercase();
    if s.is_empty() || s == "all" {
        return Ok((0..count).collect());
    }
    let mut out = Vec::new();
    for part in s.split(',') {
        let part = part.trim().replace(" to ", "-").replace(' ', "");
        let num = |t: &str| -> Result<usize, String> {
            if t == "last" || t == "end" {
                return Ok(count);
            }
            t.parse::<usize>().map_err(|_| format!("\"{t}\" isn't a page number"))
        };
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (num(a)?, num(b)?),
            None => {
                let n = num(&part)?;
                (n, n)
            }
        };
        if a == 0 || b == 0 || a > count || b > count {
            return Err(format!("page {} is out of range: it has {count}", if a == 0 || a > count { a } else { b }));
        }
        if a <= b {
            out.extend((a - 1)..b);
        } else {
            out.extend(((b - 1)..a).rev());
        }
    }
    Ok(out)
}
