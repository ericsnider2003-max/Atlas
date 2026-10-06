//! Reading inside a .zip: its list of files, and the text in them.
//!
//! **Sources:** PKWARE's *APPNOTE.TXT* (the ZIP format: end-of-central-
//! directory record, central directory headers, local headers; methods 0
//! "stored" and 8 "deflate"); RFC 1951 (DEFLATE: stored, fixed-Huffman and
//! dynamic-Huffman blocks, the length/distance tables); the canonical-Huffman
//! decoder follows Mark Adler's `puff.c` (zlib licence), which decodes a code
//! one bit at a time from the counts per length — slower than a table and
//! short enough to check by eye. CRC-32 (IEEE 802.3 polynomial, reflected)
//! checks every file that comes out. Clean-room; no zip crate.
//!
//! **Why Atlas wants it.** `index::AssetClass` classified a `.zip` and
//! stopped; `files::safe_to_unpack` was the guard for an unpacker that did
//! not exist, and `look_inside_archives` had been deleted from the settings
//! for promising one. "Find the thing about the budget" now finds it in
//! `Q3-handover.zip › budget.md`, and a zip bomb is still refused by the same
//! guard before a byte is inflated.

#[derive(Debug, Clone, PartialEq)]
struct Entry {
    name: String,
    method: u16,
    crc: u32,
    compressed: u64,
    size: u64,
    local_offset: u64,
}

fn u16le(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(i)?, *b.get(i + 1)?]))
}
fn u32le(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*b.get(i)?, *b.get(i + 1)?, *b.get(i + 2)?, *b.get(i + 3)?]))
}

/// Every file in the archive, from its central directory.
fn listing(zip: &[u8]) -> Result<Vec<Entry>, String> {
    // The end record sits in the last 22 + up to 65,535 bytes (its comment).
    let from = zip.len().saturating_sub(22 + 65_535);
    let eocd = (from..zip.len().saturating_sub(21)).rev().find(|&i| zip[i..].starts_with(b"PK\x05\x06")).ok_or("not a zip file")?;
    let count = u16le(zip, eocd + 10).ok_or("a damaged zip")? as usize;
    let mut at = u32le(zip, eocd + 16).ok_or("a damaged zip")? as usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if !zip.get(at..).is_some_and(|r| r.starts_with(b"PK\x01\x02")) {
            return Err("a damaged zip (its list of files doesn't read)".into());
        }
        let flags = u16le(zip, at + 8).ok_or("a damaged zip")?;
        let name_len = u16le(zip, at + 28).ok_or("a damaged zip")? as usize;
        let extra = u16le(zip, at + 30).ok_or("a damaged zip")? as usize;
        let comment = u16le(zip, at + 32).ok_or("a damaged zip")? as usize;
        let raw = zip.get(at + 46..at + 46 + name_len).ok_or("a damaged zip")?;
        if flags & 1 != 0 {
            return Err("it's password-protected".into());
        }
        out.push(Entry {
            name: String::from_utf8_lossy(raw).into_owned(),
            method: u16le(zip, at + 10).ok_or("a damaged zip")?,
            crc: u32le(zip, at + 16).ok_or("a damaged zip")?,
            compressed: u32le(zip, at + 20).ok_or("a damaged zip")? as u64,
            size: u32le(zip, at + 24).ok_or("a damaged zip")? as u64,
            local_offset: u32le(zip, at + 42).ok_or("a damaged zip")? as u64,
        });
        at += 46 + name_len + extra + comment;
    }
    Ok(out)
}

pub(crate) fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for b in data {
        c ^= *b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
        }
    }
    !c
}

/// One file's bytes, inflated and checked.
fn unpack(zip: &[u8], e: &Entry) -> Result<Vec<u8>, String> {
    let at = e.local_offset as usize;
    if !zip.get(at..).is_some_and(|r| r.starts_with(b"PK\x03\x04")) {
        return Err(format!("{}: damaged", e.name));
    }
    let name_len = u16le(zip, at + 26).ok_or("damaged")? as usize;
    let extra = u16le(zip, at + 28).ok_or("damaged")? as usize;
    let start = at + 30 + name_len + extra;
    let data = zip.get(start..start + e.compressed as usize).ok_or_else(|| format!("{}: cut short", e.name))?;
    let out = match e.method {
        0 => data.to_vec(),
        8 => inflate(data, e.size as usize)?,
        m => return Err(format!("{}: compressed a way I don't read (method {m})", e.name)),
    };
    if out.len() as u64 != e.size || crc32(&out) != e.crc {
        return Err(format!("{}: didn't come out as it went in", e.name));
    }
    Ok(out)
}

// ---------------------------------------------------------------- inflate

struct Bits<'a> {
    data: &'a [u8],
    pos: usize, // in bits
}

impl Bits<'_> {
    fn bit(&mut self) -> Result<u32, String> {
        let byte = *self.data.get(self.pos >> 3).ok_or("the compressed data is cut short")?;
        let b = (byte >> (self.pos & 7)) & 1;
        self.pos += 1;
        Ok(b as u32)
    }
    fn bits(&mut self, n: u32) -> Result<u32, String> {
        let mut v = 0;
        for i in 0..n {
            v |= self.bit()? << i;
        }
        Ok(v)
    }
}

/// A canonical Huffman code: how many codes of each length, and the symbols
/// in code order (`puff.c`'s representation).
struct Huffman {
    count: [u16; 16],
    symbol: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Result<Huffman, String> {
        let mut count = [0u16; 16];
        for l in lengths {
            count[*l as usize] += 1;
        }
        count[0] = 0;
        let mut offs = [0u16; 16];
        for len in 1..16 {
            offs[len] = offs[len - 1] + count[len - 1];
        }
        let mut symbol = vec![0u16; lengths.len()];
        for (s, l) in lengths.iter().enumerate() {
            if *l != 0 {
                symbol[offs[*l as usize] as usize] = s as u16;
                offs[*l as usize] += 1;
            }
        }
        Ok(Huffman { count, symbol })
    }

    fn decode(&self, b: &mut Bits) -> Result<u16, String> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= b.bit()? as i32;
            let count = self.count[len] as i32;
            if code - count < first {
                return Ok(self.symbol[(index + (code - first)) as usize]);
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err("the compressed data doesn't decode".into())
    }
}

const LEN_BASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

fn codes(b: &mut Bits, out: &mut Vec<u8>, lit: &Huffman, dist: &Huffman, limit: usize) -> Result<(), String> {
    loop {
        let sym = lit.decode(b)?;
        match sym {
            0..=255 => out.push(sym as u8),
            256 => return Ok(()),
            257..=285 => {
                let i = (sym - 257) as usize;
                let len = LEN_BASE[i] as usize + b.bits(LEN_EXTRA[i] as u32)? as usize;
                let d = dist.decode(b)? as usize;
                if d >= 30 {
                    return Err("a bad distance in the compressed data".into());
                }
                let back = DIST_BASE[d] as usize + b.bits(DIST_EXTRA[d] as u32)? as usize;
                if back > out.len() {
                    return Err("the compressed data points before its start".into());
                }
                let from = out.len() - back;
                for k in 0..len {
                    out.push(out[from + k]);
                }
            }
            _ => return Err("a bad symbol in the compressed data".into()),
        }
        if out.len() > limit {
            return Err("it inflates past what it claimed — refused".into());
        }
    }
}

/// RFC 1951 DEFLATE. `limit` is the size the archive claims; anything that
/// grows past it is refused mid-stream (the claim was checked by the guard).
pub(crate) fn inflate(data: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    let mut b = Bits { data, pos: 0 };
    let mut out = Vec::with_capacity(limit.min(64 << 20));
    loop {
        let last = b.bit()?;
        match b.bits(2)? {
            0 => {
                b.pos = (b.pos + 7) & !7;
                let i = b.pos >> 3;
                let len = u16le(data, i).ok_or("cut short")? as usize;
                let nlen = u16le(data, i + 2).ok_or("cut short")? as usize;
                if len != (!nlen & 0xFFFF) {
                    return Err("a stored block's length doesn't check".into());
                }
                out.extend_from_slice(data.get(i + 4..i + 4 + len).ok_or("cut short")?);
                b.pos = (i + 4 + len) << 3;
            }
            1 => {
                let mut l = [0u8; 288];
                l[..144].fill(8);
                l[144..256].fill(9);
                l[256..280].fill(7);
                l[280..].fill(8);
                codes(&mut b, &mut out, &Huffman::new(&l)?, &Huffman::new(&[5u8; 30])?, limit)?;
            }
            2 => {
                let nlen = b.bits(5)? as usize + 257;
                let ndist = b.bits(5)? as usize + 1;
                let ncode = b.bits(4)? as usize + 4;
                const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
                let mut cl = [0u8; 19];
                for k in ORDER.iter().take(ncode) {
                    cl[*k] = b.bits(3)? as u8;
                }
                let clh = Huffman::new(&cl)?;
                let mut lengths: Vec<u8> = Vec::with_capacity(nlen + ndist);
                while lengths.len() < nlen + ndist {
                    let sym = clh.decode(&mut b)?;
                    match sym {
                        0..=15 => lengths.push(sym as u8),
                        16 => {
                            let prev = *lengths.last().ok_or("a repeat with nothing before it")?;
                            for _ in 0..3 + b.bits(2)? {
                                lengths.push(prev);
                            }
                        }
                        17 => lengths.extend(std::iter::repeat_n(0, 3 + b.bits(3)? as usize)),
                        _ => lengths.extend(std::iter::repeat_n(0, 11 + b.bits(7)? as usize)),
                    }
                }
                if lengths.len() > nlen + ndist {
                    return Err("the code lengths run over".into());
                }
                let lit = Huffman::new(&lengths[..nlen])?;
                let dist = Huffman::new(&lengths[nlen..])?;
                codes(&mut b, &mut out, &lit, &dist, limit)?;
            }
            _ => return Err("a block of a kind that doesn't exist".into()),
        }
        if last == 1 {
            return Ok(out);
        }
        if out.len() > limit {
            return Err("it inflates past what it claimed — refused".into());
        }
    }
}

/// The first file whose name `wanted` accepts, inflated and checked, if it is
/// no bigger than `max` bytes. Used to read an app's `Info.plist` and its
/// provisioning profile out of an `.ipa` without unpacking the whole app.
pub fn file_inside(zip: &[u8], wanted: impl Fn(&str) -> bool, max: u64) -> Result<Option<(String, Vec<u8>)>, String> {
    for e in listing(zip)? {
        if e.name.ends_with('/') || !wanted(&e.name) {
            continue;
        }
        if e.size > max {
            return Err(format!("{} is {} bytes, more than the {max} it may be", e.name, e.size));
        }
        return Ok(Some((e.name.clone(), unpack(zip, &e)?)));
    }
    Ok(None)
}

/// Text files that can be searched, from inside an archive. The archive's
/// claimed total goes through `files::safe_to_unpack` first — a zip bomb is
/// listed, not inflated.
pub fn texts_inside(zip: &[u8], cfg: &crate::files::FilesConfig, max_each: u64) -> Result<Vec<(String, String)>, String> {
    let list = listing(zip)?;
    // Rounded up: 400 KB claims 1 MB, not 0, so the limit can't be slipped under.
    let claimed_mb = list.iter().map(|e| e.size).sum::<u64>().div_ceil(1024 * 1024);
    crate::files::safe_to_unpack((zip.len() / (1024 * 1024)) as u64, claimed_mb, 1, cfg)?;
    const TEXT: &[&str] = &["txt", "md", "csv", "json", "yaml", "yml", "toml", "rs", "py", "js", "ts", "html", "htm", "xml", "ini", "cfg", "log", "tex", "rst"];
    let mut out = Vec::new();
    for e in list.iter().filter(|e| !e.name.ends_with('/') && e.size <= max_each) {
        let ext = e.name.rsplit('.').next().unwrap_or("").to_lowercase();
        if !TEXT.contains(&ext.as_str()) {
            continue;
        }
        if let Ok(bytes) = unpack(zip, e) {
            if let Ok(t) = String::from_utf8(bytes) {
                out.push((e.name.clone(), t));
            }
        }
    }
    Ok(out)
}
