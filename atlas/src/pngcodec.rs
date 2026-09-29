//! Reading and writing PNG, in house.
//!
//! **Sources:** the W3C PNG specification (2nd edition): the signature, the
//! chunk layout (length, type, data, CRC-32), IHDR, PLTE, tRNS, IDAT, IEND,
//! the five scanline filters and the Paeth predictor (§9), and zlib's framing
//! (RFC 1950: a two-byte header, DEFLATE, then an Adler-32 of the raw bytes).
//! DEFLATE is the in-house inflater `zipread` already has; CRC-32 is its too.
//! Clean-room; no image crate.
//!
//! **Why Atlas wants it.** Animations were SVG only: a raster or a video needed
//! frames, and frames come back from a browser as PNG screenshots. Reading
//! them here is what lets Atlas build a GIF itself (`gifenc`) and draw a 3-D
//! scene straight to a picture (`scene3d`) with nothing installed.
//!
//! Reads 8-bit greyscale, grey+alpha, RGB, RGBA and palette images (bit depths
//! 1, 2, 4 and 8 for palette and greyscale), non-interlaced — what every
//! browser and renderer writes. Writes RGBA with stored (uncompressed) DEFLATE
//! blocks: larger files, but always correct and read by everything.

/// An image as 8-bit RGBA, row by row, top to bottom.
#[derive(Debug, Clone, PartialEq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Rgba {
    pub fn new(width: u32, height: u32) -> Rgba {
        Rgba { width, height, pixels: vec![0; (width * height * 4) as usize] }
    }

    /// The pixel at (x, y) as [r, g, b, a].
    pub fn at(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [self.pixels[i], self.pixels[i + 1], self.pixels[i + 2], self.pixels[i + 3]]
    }

    pub fn put(&mut self, x: u32, y: u32, p: [u8; 4]) {
        let i = ((y * self.width + x) * 4) as usize;
        self.pixels[i..i + 4].copy_from_slice(&p);
    }
}

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// Refused past this many pixels: a PNG says its own size, and a hostile one
/// can claim a size that would take the machine's memory.
const MAX_PIXELS: u64 = 64 << 20;

fn be32(b: &[u8], i: usize) -> Option<u32> {
    b.get(i..i + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

/// Read a PNG into RGBA.
pub fn read_png(bytes: &[u8]) -> Result<Rgba, String> {
    if !bytes.starts_with(&SIGNATURE) {
        return Err("that isn't a PNG".into());
    }
    let mut at = 8;
    let (mut w, mut h, mut depth, mut colour, mut interlace) = (0u32, 0u32, 0u8, 0u8, 0u8);
    let mut palette: Vec<[u8; 4]> = Vec::new();
    let mut idat = Vec::new();
    let mut seen_header = false;
    while at + 8 <= bytes.len() {
        let len = be32(bytes, at).ok_or("cut short")? as usize;
        let kind = &bytes[at + 4..at + 8];
        let body = bytes.get(at + 8..at + 8 + len).ok_or("a chunk runs past the end of the file")?;
        let crc = be32(bytes, at + 8 + len).ok_or("cut short")?;
        if crate::zipread::crc32(&bytes[at + 4..at + 8 + len]) != crc {
            return Err(format!("the {} chunk is damaged", String::from_utf8_lossy(kind)));
        }
        match kind {
            b"IHDR" => {
                if len < 13 {
                    return Err("a damaged header".into());
                }
                w = be32(body, 0).unwrap_or(0);
                h = be32(body, 4).unwrap_or(0);
                depth = body[8];
                colour = body[9];
                interlace = body[12];
                seen_header = true;
            }
            b"PLTE" => {
                palette = body.chunks(3).filter(|c| c.len() == 3).map(|c| [c[0], c[1], c[2], 255]).collect();
            }
            b"tRNS" if colour == 3 => {
                for (i, a) in body.iter().enumerate() {
                    if let Some(p) = palette.get_mut(i) {
                        p[3] = *a;
                    }
                }
            }
            b"IDAT" => idat.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
        at += 12 + len;
    }
    if !seen_header || w == 0 || h == 0 {
        return Err("the PNG has no size".into());
    }
    if (w as u64) * (h as u64) > MAX_PIXELS {
        return Err(format!("{w}×{h} is larger than Atlas will open"));
    }
    if interlace != 0 {
        return Err("interlaced PNGs aren't read here".into());
    }
    let channels: usize = match colour {
        0 => 1,
        2 => 3,
        3 => 1,
        4 => 2,
        6 => 4,
        _ => return Err(format!("unknown PNG colour type {colour}")),
    };
    let ok_depth = match colour {
        0 | 3 => matches!(depth, 1 | 2 | 4 | 8),
        _ => depth == 8,
    };
    if !ok_depth {
        return Err(format!("{depth}-bit PNGs of this kind aren't read here"));
    }
    if colour == 3 && palette.is_empty() {
        return Err("a palette image with no palette".into());
    }
    // zlib: two header bytes, raw DEFLATE, then the Adler-32.
    if idat.len() < 6 || (u16::from_be_bytes([idat[0], idat[1]]) % 31) != 0 || idat[0] & 0x0F != 8 {
        return Err("the image data isn't zlib".into());
    }
    let bits_per_pixel = channels * depth as usize;
    let stride = (w as usize * bits_per_pixel).div_ceil(8);
    let bpp = bits_per_pixel.div_ceil(8).max(1);
    let expect = (stride + 1) * h as usize;
    let raw = crate::zipread::inflate(&idat[2..], expect)?;
    if raw.len() < expect {
        return Err("the image data is cut short".into());
    }
    if let Some(sum) = be32(&idat, idat.len() - 4) {
        if adler32(&raw[..expect]) != sum {
            return Err("the image data is damaged (checksum)".into());
        }
    }

    // Undo the filters, a row at a time, against the row above.
    let mut rows = vec![0u8; stride * h as usize];
    for y in 0..h as usize {
        let filter = raw[y * (stride + 1)];
        let src = &raw[y * (stride + 1) + 1..(y + 1) * (stride + 1)];
        let (done, rest) = rows.split_at_mut(y * stride);
        let prev: &[u8] = if y == 0 { &[] } else { &done[(y - 1) * stride..] };
        let cur = &mut rest[..stride];
        for i in 0..stride {
            let a = if i >= bpp { cur[i - bpp] as i16 } else { 0 };
            let b = if y > 0 { prev[i] as i16 } else { 0 };
            let c = if y > 0 && i >= bpp { prev[i - bpp] as i16 } else { 0 };
            let add = match filter {
                0 => 0,
                1 => a,
                2 => b,
                3 => (a + b) / 2,
                4 => paeth(a, b, c),
                f => return Err(format!("unknown PNG filter {f}")),
            };
            cur[i] = src[i].wrapping_add(add as u8);
        }
    }

    let mut out = Rgba::new(w, h);
    for y in 0..h as usize {
        let row = &rows[y * stride..(y + 1) * stride];
        for x in 0..w as usize {
            let p = match (colour, depth) {
                (0, 8) => [row[x], row[x], row[x], 255],
                (0, d) => {
                    let v = sample(row, x, d);
                    let g = (v as u32 * 255 / ((1u32 << d) - 1)) as u8;
                    [g, g, g, 255]
                }
                (3, d) => {
                    let i = if d == 8 { row[x] } else { sample(row, x, d) } as usize;
                    *palette.get(i).ok_or("a pixel points past the palette")?
                }
                (4, _) => [row[2 * x], row[2 * x], row[2 * x], row[2 * x + 1]],
                (2, _) => [row[3 * x], row[3 * x + 1], row[3 * x + 2], 255],
                _ => [row[4 * x], row[4 * x + 1], row[4 * x + 2], row[4 * x + 3]],
            };
            out.put(x as u32, y as u32, p);
        }
    }
    Ok(out)
}

/// A packed sample of `d` bits (1, 2 or 4), most significant first.
fn sample(row: &[u8], x: usize, d: u8) -> u8 {
    let d = d as usize;
    let bit = x * d;
    let byte = row[bit / 8];
    let shift = 8 - d - (bit % 8);
    (byte >> shift) & ((1u8 << d) - 1)
}

/// The Paeth predictor: whichever of left, above, upper-left is nearest to
/// left + above - upper-left.
fn paeth(a: i16, b: i16, c: i16) -> i16 {
    let p = a + b - c;
    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for x in chunk {
            a += *x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    let crc = crate::zipread::crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// Write RGBA as a PNG: unfiltered rows in stored DEFLATE blocks. Bigger than
/// a compressed PNG, and read correctly by everything.
pub fn write_png(img: &Rgba) -> Vec<u8> {
    let stride = img.width as usize * 4;
    let mut raw = Vec::with_capacity((stride + 1) * img.height as usize);
    for y in 0..img.height as usize {
        raw.push(0);
        raw.extend_from_slice(&img.pixels[y * stride..(y + 1) * stride]);
    }
    // zlib header (deflate, 32K window, no dictionary, fastest), stored blocks.
    let mut z = vec![0x78, 0x01];
    let mut blocks = raw.chunks(65535).peekable();
    if blocks.peek().is_none() {
        z.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
    }
    while let Some(b) = blocks.next() {
        z.push(if blocks.peek().is_none() { 1 } else { 0 });
        let n = b.len() as u16;
        z.extend_from_slice(&n.to_le_bytes());
        z.extend_from_slice(&(!n).to_le_bytes());
        z.extend_from_slice(b);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut out = SIGNATURE.to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&img.width.to_be_bytes());
    ihdr.extend_from_slice(&img.height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_it_writes_it_reads_back_exactly() {
        let mut img = Rgba::new(37, 11);
        for y in 0..11 {
            for x in 0..37 {
                img.put(x, y, [(x * 7) as u8, (y * 23) as u8, (x * y) as u8, 200]);
            }
        }
        let back = read_png(&write_png(&img)).unwrap();
        assert_eq!(back, img);
    }

    #[test]
    fn the_paeth_predictor_picks_the_nearest_neighbour() {
        assert_eq!(paeth(10, 20, 10), 20);
        assert_eq!(paeth(20, 10, 10), 20);
        assert_eq!(paeth(5, 5, 5), 5);
    }

    #[test]
    fn a_damaged_chunk_is_refused_not_misread() {
        let mut png = write_png(&Rgba::new(4, 4));
        let n = png.len();
        png[n - 20] ^= 0xFF;
        assert!(read_png(&png).is_err());
    }
}
