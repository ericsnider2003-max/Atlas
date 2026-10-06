//! Writing animated GIFs, in house.
//!
//! **Sources:** CompuServe's GIF89a specification (W3C copy): the header, the
//! logical screen descriptor, the global colour table, the Graphic Control
//! Extension (frame delay in hundredths of a second), the image descriptor,
//! and the variable-width LZW with its clear and end codes packed
//! least-significant bit first into 255-byte sub-blocks (Appendix F). The code
//! width grows the moment the next code would not fit, as giflib's
//! `EGifCompressOutput` does. Looping is Netscape's application extension
//! ("NETSCAPE2.0", loop count 0 = forever). The palette is Heckbert's median
//! cut (1982): split the colour box with the most pixels × widest range at its
//! weighted median until there are 256. Clean-room; no gif crate.
//!
//! **Why Atlas wants it.** An animation was an SVG and, at best, one PNG still
//! (`motion::render`). A GIF is the thing you can drop into a message or a
//! document and have it move everywhere. Each frame after the first stores
//! only the rectangle that changed, so a ball crossing a still background costs
//! the ball, not the background.
#![allow(clippy::needless_range_loop, reason = "numeric kernels step through several arrays by one index; the index loop is the clear form")]

use crate::pngcodec::Rgba;

/// A frame and how long it stays up, in hundredths of a second (GIF's unit).
pub struct Frame<'a> {
    pub image: &'a Rgba,
    pub delay_cs: u16,
}

/// One histogram cell: how many pixels, where it sits (5 bits a channel),
/// and the exact sum of those pixels' colours.
#[derive(Clone, Copy)]
struct Cell {
    n: u64,
    at: [u8; 3],
    sum: [u64; 3],
}

/// Up to 256 colours chosen for these frames by median cut. Cells are 5 bits
/// a channel so the cut is fast whatever the frame size, but each palette
/// colour is the exact mean of the pixels in its box — a picture with 256
/// colours or fewer comes back exactly.
pub fn palette_for(frames: &[&Rgba]) -> Vec<[u8; 3]> {
    let mut hist = vec![Cell { n: 0, at: [0; 3], sum: [0; 3] }; 1 << 15];
    for img in frames {
        let n = (img.width * img.height) as usize;
        // Sample large frames rather than walk every pixel of every one.
        let step = (n * frames.len() / 400_000).max(1);
        for i in (0..n).step_by(step) {
            let p = &img.pixels[i * 4..i * 4 + 3];
            let k = key(p[0], p[1], p[2]);
            let c = &mut hist[k];
            c.n += 1;
            c.at = [p[0] >> 3, p[1] >> 3, p[2] >> 3];
            for ch in 0..3 {
                c.sum[ch] += p[ch] as u64;
            }
        }
    }
    let cells: Vec<Cell> = hist.into_iter().filter(|c| c.n > 0).collect();
    if cells.is_empty() {
        return vec![[0, 0, 0]];
    }
    let mut boxes: Vec<Vec<Cell>> = vec![cells];
    while boxes.len() < 256 {
        // The box that most deserves a split: pixels × its widest channel.
        let mut best = None;
        let mut best_score = 0u64;
        for (i, b) in boxes.iter().enumerate() {
            if b.len() < 2 {
                continue;
            }
            let (ch, range) = widest(b);
            let weight: u64 = b.iter().map(|c| c.n).sum();
            let score = weight * (range as u64 + 1);
            if score > best_score {
                best_score = score;
                best = Some((i, ch));
            }
        }
        let Some((i, ch)) = best else { break };
        let mut b = boxes.swap_remove(i);
        b.sort_by_key(|c| c.at[ch]);
        let total: u64 = b.iter().map(|c| c.n).sum();
        let mut run = 0u64;
        let mut cut = 1;
        for (j, c) in b.iter().enumerate() {
            run += c.n;
            if run * 2 >= total {
                cut = (j + 1).clamp(1, b.len() - 1);
                break;
            }
        }
        let rest = b.split_off(cut);
        boxes.push(b);
        boxes.push(rest);
    }
    boxes
        .iter()
        .map(|b| {
            let w: u64 = b.iter().map(|c| c.n).sum::<u64>().max(1);
            let mean = |ch: usize| -> u8 { ((b.iter().map(|c| c.sum[ch]).sum::<u64>() + w / 2) / w) as u8 };
            [mean(0), mean(1), mean(2)]
        })
        .collect()
}

fn key(r: u8, g: u8, b: u8) -> usize {
    ((r as usize >> 3) << 10) | ((g as usize >> 3) << 5) | (b as usize >> 3)
}

fn widest(b: &[Cell]) -> (usize, u8) {
    let mut best = (0, 0u8);
    for ch in 0..3 {
        let lo = b.iter().map(|c| c.at[ch]).min().unwrap_or(0);
        let hi = b.iter().map(|c| c.at[ch]).max().unwrap_or(0);
        if hi - lo >= best.1 {
            best = (ch, hi - lo);
        }
    }
    best
}

/// Maps colours to their nearest palette entry, remembering each answer.
struct Nearest<'a> {
    palette: &'a [[u8; 3]],
    memo: Vec<u16>,
}

impl<'a> Nearest<'a> {
    fn new(palette: &'a [[u8; 3]]) -> Self {
        Nearest { palette, memo: vec![u16::MAX; 1 << 18] }
    }

    fn index(&mut self, r: u8, g: u8, b: u8) -> u8 {
        // 6 bits a channel for the memo: finer than the palette was chosen at.
        let k = ((r as usize >> 2) << 12) | ((g as usize >> 2) << 6) | (b as usize >> 2);
        if self.memo[k] == u16::MAX {
            let mut best = (u32::MAX, 0usize);
            for (i, p) in self.palette.iter().enumerate() {
                let d = |a: u8, b: u8| (a as i32 - b as i32).pow(2) as u32;
                let dist = d(r, p[0]) * 2 + d(g, p[1]) * 4 + d(b, p[2]) * 3;
                if dist < best.0 {
                    best = (dist, i);
                }
            }
            self.memo[k] = best.1 as u16;
        }
        self.memo[k] as u8
    }
}

/// The smallest rectangle where `a` and `b` differ, as (x, y, w, h).
fn changed(a: &[u8], b: &[u8], width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for y in 0..height {
        for x in 0..width {
            let i = ((y * width + x) * 4) as usize;
            if a[i..i + 3] != b[i..i + 3] {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    (x0 != u32::MAX).then(|| (x0, y0, x1 - x0 + 1, y1 - y0 + 1))
}

/// Encode the frames as a looping GIF. All frames must be the same size.
pub fn encode_gif(frames: &[Frame]) -> Result<Vec<u8>, String> {
    encode(frames, false)
}

/// The same, with ordered dithering (Bayer's 8×8 matrix, 1973): smooth
/// gradients — a sky, the shading round a ball — come out as a fine even
/// grain instead of bands. Ordered rather than error-diffused because the
/// pattern depends only on the pixel's position, so a part of the picture
/// that doesn't move doesn't shimmer from frame to frame.
pub fn encode_gif_dithered(frames: &[Frame]) -> Result<Vec<u8>, String> {
    encode(frames, true)
}

const BAYER8: [[u8; 8]; 8] = [
    [0, 32, 8, 40, 2, 34, 10, 42],
    [48, 16, 56, 24, 50, 18, 58, 26],
    [12, 44, 4, 36, 14, 46, 6, 38],
    [60, 28, 52, 20, 62, 30, 54, 22],
    [3, 35, 11, 43, 1, 33, 9, 41],
    [51, 19, 59, 27, 49, 17, 57, 25],
    [15, 47, 7, 39, 13, 45, 5, 37],
    [63, 31, 55, 23, 61, 29, 53, 21],
];

fn encode(frames: &[Frame], dither: bool) -> Result<Vec<u8>, String> {
    let first = frames.first().ok_or("no frames to make a GIF from")?;
    let (w, h) = (first.image.width, first.image.height);
    if w == 0 || h == 0 || w > 65535 || h > 65535 {
        return Err(format!("a GIF can't be {w}×{h}"));
    }
    if frames.iter().any(|f| f.image.width != w || f.image.height != h) {
        return Err("the frames aren't all the same size".into());
    }
    let images: Vec<&Rgba> = frames.iter().map(|f| f.image).collect();
    let mut palette = palette_for(&images);
    let mut bits = 1u8;
    while (1usize << bits) < palette.len() {
        bits += 1;
    }
    while palette.len() < 1 << bits {
        palette.push([0, 0, 0]);
    }
    let mut nearest = Nearest::new(&palette);

    let mut out = b"GIF89a".to_vec();
    out.extend_from_slice(&(w as u16).to_le_bytes());
    out.extend_from_slice(&(h as u16).to_le_bytes());
    out.push(0x80 | ((bits - 1) << 4) | (bits - 1)); // global table, its size
    out.push(0); // background colour index
    out.push(0); // no aspect ratio
    for c in &palette {
        out.extend_from_slice(c);
    }
    // Loop forever.
    out.extend_from_slice(&[0x21, 0xFF, 0x0B]);
    out.extend_from_slice(b"NETSCAPE2.0");
    out.extend_from_slice(&[0x03, 0x01, 0x00, 0x00, 0x00]);

    let mut prev: Option<&Rgba> = None;
    let mut pending_delay = 0u32;
    for f in frames {
        let rect = match prev {
            None => Some((0, 0, w, h)),
            Some(p) => changed(&p.pixels, &f.image.pixels, w, h),
        };
        let Some((x, y, rw, rh)) = rect else {
            // Nothing moved: lengthen the frame already written instead.
            pending_delay += f.delay_cs as u32;
            extend_last_delay(&mut out, pending_delay);
            continue;
        };
        pending_delay = f.delay_cs as u32;
        // Graphic Control Extension: leave the frame in place, then this delay.
        out.extend_from_slice(&[0x21, 0xF9, 0x04, 0x04]);
        out.extend_from_slice(&f.delay_cs.to_le_bytes());
        out.extend_from_slice(&[0x00, 0x00]);
        // Image descriptor.
        out.push(0x2C);
        for v in [x, y, rw, rh] {
            out.extend_from_slice(&(v as u16).to_le_bytes());
        }
        out.push(0);
        let mut indices = Vec::with_capacity((rw * rh) as usize);
        for yy in y..y + rh {
            for xx in x..x + rw {
                let p = f.image.at(xx, yy);
                let (r, g, b) = if dither {
                    // ±6 levels around the pixel, by where it sits.
                    let o = BAYER8[(yy % 8) as usize][(xx % 8) as usize] as i32 * 12 / 63 - 6;
                    let q = |c: u8| (c as i32 + o).clamp(0, 255) as u8;
                    (q(p[0]), q(p[1]), q(p[2]))
                } else {
                    (p[0], p[1], p[2])
                };
                indices.push(nearest.index(r, g, b));
            }
        }
        let min_code = bits.max(2);
        out.push(min_code);
        lzw(&indices, min_code, &mut out);
        prev = Some(f.image);
    }
    out.push(0x3B);
    Ok(out)
}

/// Rewrite the delay in the last Graphic Control Extension written.
fn extend_last_delay(out: &mut [u8], delay: u32) {
    let d = delay.min(u16::MAX as u32) as u16;
    if let Some(at) = (0..out.len().saturating_sub(7)).rev().find(|&i| out[i] == 0x21 && out[i + 1] == 0xF9 && out[i + 2] == 0x04) {
        out[at + 4..at + 6].copy_from_slice(&d.to_le_bytes());
    }
}

/// GIF's LZW, written into 255-byte sub-blocks and a terminator.
fn lzw(indices: &[u8], min_code: u8, out: &mut Vec<u8>) {
    let clear = 1u16 << min_code;
    let end = clear + 1;
    let mut size = min_code as u32 + 1;
    let mut next = end + 1;
    let mut dict: std::collections::HashMap<u32, u16> = std::collections::HashMap::new();
    let mut packed: Vec<u8> = Vec::new();
    let (mut acc, mut nbits) = (0u32, 0u32);
    let mut emit = |code: u16, size: u32, packed: &mut Vec<u8>| {
        acc |= (code as u32) << nbits;
        nbits += size;
        while nbits >= 8 {
            packed.push(acc as u8);
            acc >>= 8;
            nbits -= 8;
        }
    };
    emit(clear, size, &mut packed);
    let mut it = indices.iter();
    if let Some(&first) = it.next() {
        let mut prefix = first as u16;
        for &k in it {
            let entry = ((prefix as u32) << 8) | k as u32;
            if let Some(&c) = dict.get(&entry) {
                prefix = c;
                continue;
            }
            emit(prefix, size, &mut packed);
            if next as u32 >= (1 << size) && size < 12 {
                size += 1;
            }
            if next >= 4095 {
                emit(clear, size, &mut packed);
                dict.clear();
                size = min_code as u32 + 1;
                next = end + 1;
            } else {
                dict.insert(entry, next);
                next += 1;
            }
            prefix = k as u16;
        }
        emit(prefix, size, &mut packed);
        if next as u32 >= (1 << size) && size < 12 {
            size += 1;
        }
    }
    emit(end, size, &mut packed);
    if nbits > 0 {
        packed.push(acc as u8);
    }
    for block in packed.chunks(255) {
        out.push(block.len() as u8);
        out.extend_from_slice(block);
    }
    out.push(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_two_colour_picture_keeps_both_colours() {
        let mut img = Rgba::new(8, 8);
        for y in 0..8 {
            for x in 0..8 {
                img.put(x, y, if x < 4 { [255, 0, 0, 255] } else { [0, 0, 255, 255] });
            }
        }
        let pal = palette_for(&[&img]);
        assert!(pal.contains(&[255, 0, 0]) && pal.contains(&[0, 0, 255]), "{pal:?}");
    }

    #[test]
    fn only_the_rectangle_that_changed_is_found() {
        let a = Rgba::new(10, 10);
        let mut b = a.clone();
        b.put(3, 4, [9, 9, 9, 255]);
        b.put(6, 7, [9, 9, 9, 255]);
        assert_eq!(changed(&a.pixels, &b.pixels, 10, 10), Some((3, 4, 4, 4)));
        assert_eq!(changed(&a.pixels, &a.pixels, 10, 10), None);
    }
}
