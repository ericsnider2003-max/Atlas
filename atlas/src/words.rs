//! Reading the words off a screen, in Atlas's own code.
//!
//! ## What this replaces, and why
//!
//! `ocr` shells out to `tesseract.exe`: a separate program, a separate
//! install, a separate thing to go missing. It has never run, because nothing
//! in `ATLAS.bat` ever downloaded it — the capability page has said *"waiting
//! on tesseract"* since the day it was written.
//!
//! This is the same job done in two model files and this module. No second
//! process, no PATH, no install, nothing to go out of date separately from
//! Atlas. The same arrangement as everything else Atlas sees with: **our
//! code, somebody's weights, downloaded once.**
//!
//! ## Two models, because it is two jobs
//!
//! Finding *where* the words are and reading *what they say* are different
//! problems and are not solved by one network.
//!
//! 1. **The finder** turns a whole picture into a map of how likely each pixel
//!    is to be part of a letter. It has no idea what the letters are.
//! 2. **The reader** turns one strip of picture into characters. It has no
//!    idea where the strip came from and cannot find a second one.
//!
//! Everything between those two — turning a probability map into boxes, and
//! turning a box into a strip the reader will accept — is this file, and it is
//! where the accuracy is won or lost.
//!
//! ## The part that is deliberately narrower than the model allows
//!
//! Boxes here are **upright rectangles**. The finder can support text at any
//! angle, and doing that properly means fitting a minimum-area rotated
//! rectangle to each blob and then offsetting a polygon outward — a
//! respectable amount of geometry, most of which has no test that can be
//! written without a photograph.
//!
//! This reads screens. Text on a screen is upright, near enough always. So a
//! rotated line here is read as the upright box around it, which for a slight
//! tilt is fine and for a real rotation is poor — and that is written down
//! rather than discovered. If Atlas ever needs to read a photograph of a
//! street sign, this is the thing to come back to.
//!
//! ## The failure this is built to avoid
//!
//! OCR does not fail by returning an error. It fails by returning
//! **confident, plausible, wrong text** — and an assistant that acts on
//! plausible wrong text is worse than one that says it could not read the
//! screen. So every strip carries its own number, the whole reading carries
//! one, and `worth_acting_on` exists so a caller has to pass through a
//! judgement rather than reach straight for the string.

use crate::error::{AtlasError, Result};
use crate::infer::{Kind, Model, Outputs};
use crate::vision::Patch;
use serde::Deserialize;
use std::path::Path;

/// What the reader can say.
///
/// Thirty-six characters, and the model returns lower case whatever it saw —
/// it was trained to. So a heading in capitals comes back in lower case, and
/// that is the model's limit rather than a bug to look for here.
pub const ALPHABET: &str = "0123456789abcdefghijklmnopqrstuvwxyz";

/// One more than the alphabet: the blank the recogniser emits between letters.
pub const CLASSES: usize = 37;

/// Said out loud, because the limit surprises people.
pub const NO_CAPITALS: &str =
    "the reader returns lower case whatever it saw — the English model was trained on \
     thirty-six characters and capitals are not among them";

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(default)]
pub struct WordsConfig {
    /// A pixel is part of a letter above this.
    pub ink: f32,
    /// A box has to average this much to be kept. Higher than `ink` on
    /// purpose — see `find`.
    pub keep_above: f32,
    /// How far a box is grown back after it is found.
    pub grow_by: f32,
    /// Most boxes to read. Reading is the slow half, and a screen that
    /// produces four hundred boxes has found noise, not text.
    pub most: usize,
    /// Boxes thinner or shorter than this in finder pixels are skipped.
    pub smallest: usize,
    /// Words the reader is less sure of than this are dropped.
    pub sure_enough: f32,
}

impl Default for WordsConfig {
    fn default() -> Self {
        WordsConfig {
            ink: 0.3,
            keep_above: 0.5,
            grow_by: 2.0,
            most: 200,
            smallest: 2,
            sure_enough: 0.5,
        }
    }
}

// ---------------------------------------------------------------------------
// Finding where the words are
// ---------------------------------------------------------------------------

/// A box on the finder's map, in the finder's own pixels.
///
/// Not a `Patch`. A `Patch` is a fraction of a frame and this is a rectangle
/// of a 736-pixel square, and the two have been the same type in enough
/// codebases for the conversion to get skipped exactly once.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Strip {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    /// The average probability inside it. Its case for existing.
    pub strength: f32,
}

impl Strip {
    fn area(&self) -> usize {
        self.width * self.height
    }
}

/// Every lump of ink on the map, as upright boxes.
///
/// Eight-connected and scanned row by row, so the order out is fixed. A
/// four-connected version splits a diagonal stroke into two boxes and reads
/// one letter as two.
pub fn blobs(map: &[f32], width: usize, height: usize, ink: f32) -> Vec<Strip> {
    let mut found = Vec::new();
    if width == 0 || height == 0 || map.len() < width * height {
        return found;
    }
    let mut seen = vec![false; width * height];
    // An explicit stack rather than recursion: a screenshot of solid text is
    // one blob of half a million pixels, and that is a blown stack rather than
    // a slow answer.
    let mut stack: Vec<usize> = Vec::new();

    for start in 0..width * height {
        if seen[start] || map[start] < ink {
            continue;
        }
        stack.clear();
        stack.push(start);
        seen[start] = true;
        let (mut x0, mut x1) = (start % width, start % width);
        let (mut y0, mut y1) = (start / width, start / width);

        while let Some(at) = stack.pop() {
            let (ax, ay) = (at % width, at / width);
            x0 = x0.min(ax);
            x1 = x1.max(ax);
            y0 = y0.min(ay);
            y1 = y1.max(ay);
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let nx = ax as i64 + dx;
                    let ny = ay as i64 + dy;
                    if nx < 0 || ny < 0 || nx >= width as i64 || ny >= height as i64 {
                        continue;
                    }
                    let n = ny as usize * width + nx as usize;
                    if !seen[n] && map[n] >= ink {
                        seen[n] = true;
                        stack.push(n);
                    }
                }
            }
        }
        found.push(Strip {
            x: x0,
            y: y0,
            width: x1 - x0 + 1,
            height: y1 - y0 + 1,
            strength: 0.0,
        });
    }
    found
}

/// The average probability inside a box.
pub fn strength(map: &[f32], width: usize, s: &Strip) -> f32 {
    if s.width == 0 || s.height == 0 {
        return 0.0;
    }
    let mut total = 0.0f32;
    for y in s.y..s.y + s.height {
        for x in s.x..s.x + s.width {
            total += map.get(y * width + x).copied().unwrap_or(0.0);
        }
    }
    total / s.area() as f32
}

/// Grow a box back out.
///
/// The finder was trained on text regions that had been **shrunk**, so what it
/// reports is smaller than the letters. Growing it back by
/// `area × ratio ÷ perimeter` is the standard undo, and for a rectangle that
/// formula is exact rather than an approximation — which is the one real
/// argument for keeping boxes upright.
///
/// Leaving this out does not fail. It crops the tops off tall letters and the
/// tails off descenders, and hands back words that are *nearly* right.
pub fn grow(s: &Strip, ratio: f32, width: usize, height: usize) -> Strip {
    let perimeter = 2 * (s.width + s.height);
    if perimeter == 0 {
        return *s;
    }
    let d = ((s.area() as f32 * ratio) / perimeter as f32).round() as usize;
    let x = s.x.saturating_sub(d);
    let y = s.y.saturating_sub(d);
    let right = (s.x + s.width + d).min(width);
    let bottom = (s.y + s.height + d).min(height);
    Strip {
        x,
        y,
        width: right.saturating_sub(x),
        height: bottom.saturating_sub(y),
        strength: s.strength,
    }
}

/// Rows top to bottom, and left to right within a row.
///
/// Sorting by `y` alone puts the second word of a line before the first
/// whenever it happens to sit a pixel higher, which is most of the time — and
/// the result is text that reads as a shuffled word list while looking like it
/// worked.
pub fn reading_order(mut strips: Vec<Strip>) -> Vec<Strip> {
    strips.sort_by(|a, b| a.y.cmp(&b.y).then(a.x.cmp(&b.x)));
    let mut rows: Vec<Vec<Strip>> = Vec::new();
    for s in strips {
        let mut placed = false;
        for row in rows.iter_mut() {
            // Against the row's whole span, not against its first member: a
            // row grows downward as tall letters join it, and a comma at the
            // end of a long line overlaps the line but not its first word.
            let top = row.iter().map(|r| r.y).min().unwrap_or(s.y);
            let bottom = row.iter().map(|r| r.y + r.height).max().unwrap_or(s.y + s.height);
            if shares_a_line(top, bottom, s.y, s.y + s.height) {
                row.push(s);
                placed = true;
                break;
            }
        }
        if !placed {
            rows.push(vec![s]);
        }
    }
    let mut out = Vec::new();
    for mut row in rows {
        row.sort_by_key(|r| r.x);
        out.append(&mut row);
    }
    out
}

/// Do two vertical spans overlap by more than half the shorter one?
///
/// The arithmetic behind the row grouping. By overlap rather than by distance
/// between centres: a full stop and the word before it have very different
/// heights and the same line.
pub fn shares_a_line(top_a: usize, bottom_a: usize, top_b: usize, bottom_b: usize) -> bool {
    let top = top_a.max(top_b);
    let bottom = bottom_a.min(bottom_b);
    if bottom <= top {
        return false;
    }
    let shared = bottom - top;
    let shorter = bottom_a.saturating_sub(top_a).min(bottom_b.saturating_sub(top_b));
    shared * 2 > shorter
}

/// Everything: a probability map in, boxes out, in reading order.
///
/// The order of operations matters and is not the obvious one. Each box is
/// **scored before it is grown**. Scoring the grown box averages in the
/// background it has just swallowed, which drags every score down — and drags
/// it furthest for the smallest text, so the first thing it would silently
/// throw away is fine print.
pub fn find(map: &[f32], width: usize, height: usize, cfg: &WordsConfig) -> Located {
    let mut kept: Vec<Strip> = blobs(map, width, height, cfg.ink)
        .into_iter()
        .filter(|s| s.width >= cfg.smallest && s.height >= cfg.smallest)
        .map(|s| Strip { strength: strength(map, width, &s), ..s })
        .filter(|s| s.strength >= cfg.keep_above)
        .map(|s| grow(&s, cfg.grow_by, width, height))
        .collect();
    // Strongest first, then cut, then put back into reading order. Cutting by
    // position instead would drop the bottom of the screen rather than the
    // least convincing part of it.
    kept.sort_by(|a, b| b.strength.total_cmp(&a.strength));
    let over_the_limit = kept.len().saturating_sub(cfg.most.max(1));
    kept.truncate(cfg.most.max(1));
    Located { strips: reading_order(kept), over_the_limit }
}

/// Where the words are, and how many did not fit.
///
/// Named `Located` rather than the obvious `Found` because four other modules
/// in this codebase already define a `Found` — `firstrun`, `reference`,
/// `onlyone` and one more — and near-identical type names across modules is
/// one of the four ways this codebase has broken before. A fifth would have
/// compiled perfectly.
///
/// The count is separate from the boxes because the two mean different things
/// and were briefly one number here: "boxes the finder produced that I did not
/// return" would have counted noise the threshold correctly rejected alongside
/// real text the limit cut off, and reported a healthy screen as a screen
/// Atlas could not keep up with.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Located {
    pub strips: Vec<Strip>,
    /// Boxes that passed every test and were still cut, because there were
    /// more than `most` of them. Real text, not read.
    pub over_the_limit: usize,
}

// ---------------------------------------------------------------------------
// Turning a strip into letters
// ---------------------------------------------------------------------------

/// What the reader made of one strip.
#[derive(Debug, Clone, PartialEq)]
pub struct Letters {
    pub text: String,
    /// How sure it was, averaged over the steps that produced a character.
    ///
    /// Averaged over those steps only, not over all of them. A short word in a
    /// long window is mostly blank, and blanks are the easy part — including
    /// them would report ninety per cent for a word that was a guess.
    pub sure: f32,
}

/// Are these already probabilities, or raw scores?
///
/// Asked rather than assumed. Softmaxing something already softmaxed does not
/// fail: it flattens everything toward one thirty-seventh and reports four per
/// cent for a word that was read perfectly. The text would be right and the
/// number beside it wrong, which is the worse of the two — a low number gets
/// the reading thrown away by `sure_enough`, so the failure looks like "Atlas
/// can't read my screen" rather than like a bug.
pub fn looks_like_odds(values: &[f32], steps: usize, classes: usize) -> bool {
    for t in 0..steps {
        let row = &values[t * classes..(t + 1) * classes];
        if row.iter().any(|v| *v < -0.001 || *v > 1.001) {
            return false;
        }
        if (row.iter().sum::<f32>() - 1.0).abs() > 0.01 {
            return false;
        }
    }
    true
}

/// Greedy CTC: the best class at each step, blanks dropped, repeats collapsed.
///
/// Blank is class nought and the alphabet starts at one, which is how the
/// model was exported. A repeat is only collapsed when nothing separates the
/// two — a blank between them resets it, which is the whole reason the blank
/// exists and is why "cool" keeps both of its o's.
pub fn ctc(values: &[f32], steps: usize) -> Result<Letters> {
    if steps == 0 || values.len() < steps * CLASSES {
        return Err(AtlasError::Platform(format!(
            "the reader gave {} numbers for {steps} step(s), and {} characters need {} of them \
             — that isn't the model I was written for",
            values.len(),
            CLASSES,
            steps * CLASSES
        )));
    }
    let already = looks_like_odds(values, steps, CLASSES);
    let letters: Vec<char> = ALPHABET.chars().collect();

    let mut text = String::new();
    let mut picked: Vec<f32> = Vec::new();
    let mut last = usize::MAX;

    for t in 0..steps {
        let row = &values[t * CLASSES..(t + 1) * CLASSES];
        let mut best = 0usize;
        for (i, v) in row.iter().enumerate() {
            if *v > row[best] {
                best = i;
            }
        }
        if best != 0
            && best != last {
                // `best - 1` because class nought is the blank. Getting this
                // off by one does not fail — it returns real words with every
                // letter shifted, which reads as a broken model rather than as
                // an index.
                text.push(letters[best - 1]);
                picked.push(if already {
                    row[best]
                } else {
                    let top = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                    let total: f32 = row.iter().map(|v| (v - top).exp()).sum();
                    if total > 0.0 {
                        (row[best] - top).exp() / total
                    } else {
                        0.0
                    }
                });
            }
        last = best;
    }

    let sure = if picked.is_empty() {
        0.0
    } else {
        picked.iter().sum::<f32>() / picked.len() as f32
    };
    Ok(Letters { text, sure })
}

// ---------------------------------------------------------------------------
// A whole screenful
// ---------------------------------------------------------------------------

/// One box and what it said, in fractions of the original frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Lettering {
    pub at: Patch,
    pub text: String,
    pub sure: f32,
}

/// Everything read off one picture.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Screenful {
    pub lettering: Vec<Lettering>,
    /// Boxes that were found and read, and then thrown away for being a
    /// guess. Counted rather than discarded silently: a screen where forty
    /// were kept and two hundred dropped is a screen Atlas did not read, and
    /// that is worth knowing.
    pub unsure: usize,
    /// Boxes the finder produced that were never read, because `most` cut
    /// them off.
    pub unread: usize,
}

impl Screenful {
    /// The text, in reading order, one line per row.
    pub fn text(&self) -> String {
        self.lettering
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn words(&self) -> usize {
        self.lettering.len()
    }

    /// How sure it is overall.
    ///
    /// Weighted by how long each word is, so one confident "a" does not
    /// outvote a doubtful eleven-character one.
    pub fn sure(&self) -> f32 {
        let total: usize = self.lettering.iter().map(|l| l.text.chars().count()).sum();
        if total == 0 {
            return 0.0;
        }
        self.lettering
            .iter()
            .map(|l| l.sure * l.text.chars().count() as f32)
            .sum::<f32>()
            / total as f32
    }

    /// Should a caller use this?
    ///
    /// Here rather than left to each caller, because the way this goes wrong
    /// is one caller out of six forgetting to ask. Reading a screen and acting
    /// on plausible nonsense is worse than saying the screen could not be
    /// read.
    pub fn worth_acting_on(&self) -> bool {
        self.words() >= 2 && self.sure() >= 0.7 && self.unsure <= self.words()
    }

    pub fn spoken(&self) -> String {
        if self.words() == 0 {
            return "I couldn't find any words there.".into();
        }
        if !self.worth_acting_on() {
            return format!(
                "I could only half-read that — {} word{} at {:.0}%{}. I'd rather not act on it.",
                self.words(),
                if self.words() == 1 { "" } else { "s" },
                self.sure() * 100.0,
                if self.unsure > 0 {
                    format!(", with {} more too unclear to keep", self.unsure)
                } else {
                    String::new()
                }
            );
        }
        let mut said = format!("Read {} words at {:.0}%.", self.words(), self.sure() * 100.0);
        if self.unread > 0 {
            said.push_str(&format!(
                " {} more were found and not read — there was more on screen than I look at in \
                 one go.",
                self.unread
            ));
        }
        said
    }
}

// ---------------------------------------------------------------------------
// The two models, together
// ---------------------------------------------------------------------------

/// Both models, loaded once.
///
/// Both or neither. A finder with no reader is a thing that knows where the
/// words are and cannot say one of them, and offering that as a working
/// feature is how this codebase has produced modules that pass their tests and
/// do nothing.
pub struct Reader {
    finder: Model,
    reader: Model,
}

impl std::fmt::Debug for Reader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Reader { finder, reader }")
    }
}

impl Reader {
    pub fn open(models_dir: &Path) -> Result<Reader> {
        Ok(Reader {
            finder: Model::load(Kind::TextFind, models_dir)?,
            reader: Model::load(Kind::TextRead, models_dir)?,
        })
    }

    /// Is this even worth trying?
    pub fn installed(models_dir: &Path) -> bool {
        crate::infer::whats_missing(models_dir, &Kind::for_reading()).is_empty()
    }

    /// Read one picture.
    ///
    /// `rgb` is three bytes per pixel, red first — a screen grab or a camera
    /// frame, whichever the caller has.
    pub fn look(&mut self, rgb: &[u8], w: usize, h: usize, cfg: &WordsConfig) -> Result<Screenful> {
        // Letterboxed rather than stretched to the square. A 2560×1392 screen
        // squeezed into 736×736 makes every character half as wide as it was
        // trained on, and the finder does not report that as a problem — it
        // just finds fewer words. Grey bars cost nothing and keep the shapes.
        let (prepared, placed) = crate::vision::letterbox(rgb, w, h, Kind::TextFind);
        let out = self.finder.run(&prepared)?;
        let map = one_map(&out)?;
        let (fw, fh) = Kind::TextFind.wants();
        if map.len() < fw * fh {
            return Err(AtlasError::Platform(format!(
                "the finder returned {} values and a {fw}×{fh} map needs {} — that isn't the \
                 model I was written for",
                map.len(),
                fw * fh
            )));
        }

        let found = find(map, fw, fh, cfg);
        let mut lettering = Vec::new();
        let mut unsure = 0usize;
        for s in &found.strips {
            let at = placed.back(s.x as f32, s.y as f32, s.width as f32, s.height as f32);
            // Cropped out of the ORIGINAL picture, not out of the letterboxed
            // copy. The letterbox is 736 wide and a line of text on a 2560
            // screen is a fraction of that — reading the shrunk copy would
            // hand the reader a strip that has already lost most of itself.
            let Some(area) = crop_in(&at, w, h) else { continue };
            // One word at a time. The finder hands back whole lines, and the
            // reader takes a strip 100 wide by 32 high: a four-word line
            // squeezed into that is a quarter as wide per letter as it was
            // trained on, and "meeting moved to three" came back as
            // "metirgmoediatives" (24 Sep 2026, the first run on a real
            // picture). Split at the gaps between words first.
            let mut read = Vec::new();
            let mut sure = 1.0f32;
            for piece in words_in(rgb, w, h, area) {
                let strip = crate::infer::prepare_crop(rgb, w, h, piece, Kind::TextRead);
                if strip.is_empty() {
                    continue;
                }
                let said = self.reader.run(&strip)?;
                let values = said.only()?;
                let steps = values.len() / CLASSES;
                let letters = ctc(values, steps)?;
                if letters.text.is_empty() {
                    continue;
                }
                if letters.sure < cfg.sure_enough {
                    unsure += 1;
                    continue;
                }
                sure = sure.min(letters.sure);
                read.push(letters.text);
            }
            if read.is_empty() {
                continue;
            }
            lettering.push(Lettering { at, text: read.join(" "), sure });
        }
        Ok(Screenful { lettering, unsure, unread: found.over_the_limit })
    }
}

/// The finder's one output, checked.
///
/// It has exactly one. Saying so here means a model with three is an error
/// with a name rather than a silent read of the wrong one, which is the
/// mistake `Outputs` exists to make impossible.
fn one_map(out: &Outputs) -> Result<&[f32]> {
    if out.count() != 1 {
        return Err(AtlasError::Platform(format!(
            "the finder gave {} outputs and I expected one map — that isn't the model I was \
             written for",
            out.count()
        )));
    }
    out.only()
}

// ---------------------------------------------------------------------------
// Getting the pixels in the first place
// ---------------------------------------------------------------------------

/// Grab a rectangle of the screen as raw red-green-blue bytes.
///
/// Straight to standard output rather than to a PNG file. The old route wrote
/// a picture to disk and handed the filename to another program; this hands
/// back `w × h × 3` bytes and nothing touches the disk, which also means
/// nothing is left behind for somebody to find later.
///
/// `rawvideo` is the only output format with no decoder on the far side. That
/// matters more than it sounds: a PNG would have to be decoded, Atlas has no
/// decoder, and adding one to read a picture Atlas just asked to be encoded is
/// work done twice for no reason.
pub fn capture_args(x: i32, y: i32, w: u32, h: u32) -> Vec<String> {
    vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-f".into(),
        "gdigrab".into(),
        "-framerate".into(),
        "1".into(),
        "-offset_x".into(),
        x.to_string(),
        "-offset_y".into(),
        y.to_string(),
        "-video_size".into(),
        format!("{w}x{h}"),
        "-i".into(),
        "desktop".into(),
        "-frames:v".into(),
        "1".into(),
        "-f".into(),
        "rawvideo".into(),
        "-pix_fmt".into(),
        "rgb24".into(),
        "-".into(),
    ]
}

/// Decode a picture file to raw red-green-blue bytes.
///
/// The size has to be known first — see `picture_size`. Without it there is no
/// way to tell a short read from a complete one, and a short read is a picture
/// with the bottom missing that reads perfectly and is wrong.
pub fn picture_args(path: &str) -> Vec<String> {
    vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-i".into(),
        path.into(),
        "-f".into(),
        "rawvideo".into(),
        "-pix_fmt".into(),
        "rgb24".into(),
        "-".into(),
    ]
}

/// How big a picture is, from its first few bytes.
///
/// Both PNG and JPEG carry their dimensions in the header, uncompressed, which
/// means this needs no decoder and no dependency — forty lines against a crate
/// and a build.
///
/// It is needed because of what happens without it. `ffmpeg` will happily hand
/// back a stream of bytes, and with no expected count a truncated one is
/// indistinguishable from a whole one: Atlas would read the top two-thirds of
/// the screen, find words, and never mention the third it did not get.
pub fn picture_size(head: &[u8]) -> Option<(usize, usize)> {
    // PNG: the eight-byte signature, then a chunk header, then IHDR's width
    // and height as big-endian fours. Always at the same offsets — the
    // standard requires IHDR to come first.
    const PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if head.len() >= 24 && head[..8] == PNG && &head[12..16] == b"IHDR" {
        let be = |at: usize| -> usize {
            u32::from_be_bytes([head[at], head[at + 1], head[at + 2], head[at + 3]]) as usize
        };
        let (w, h) = (be(16), be(20));
        return if w > 0 && h > 0 { Some((w, h)) } else { None };
    }

    // JPEG: walk the markers to the start-of-frame, which carries the size.
    // Every marker is 0xFF then a byte; most then carry a big-endian length
    // that includes its own two bytes.
    if head.len() >= 4 && head[0] == 0xFF && head[1] == 0xD8 {
        let mut at = 2usize;
        while at + 3 < head.len() {
            if head[at] != 0xFF {
                // Padding between markers is legal and is always 0xFF, so
                // anything else here means this is not a JPEG after all.
                return None;
            }
            let marker = head[at + 1];
            at += 2;
            // These stand alone and carry no length.
            if marker == 0xD8 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
                continue;
            }
            if marker == 0xD9 || marker == 0xDA {
                return None; // end, or the compressed data — no size here
            }
            if at + 1 >= head.len() {
                return None;
            }
            let len = u16::from_be_bytes([head[at], head[at + 1]]) as usize;
            // Every start-of-frame marker, not just the baseline one. A
            // progressive JPEG is 0xC2 and is otherwise identical here;
            // checking only 0xC0 fails on exactly the pictures a phone makes.
            let start_of_frame = (0xC0..=0xCF).contains(&marker)
                && marker != 0xC4
                && marker != 0xC8
                && marker != 0xCC;
            if start_of_frame {
                if at + 7 >= head.len() {
                    return None;
                }
                let h_px = u16::from_be_bytes([head[at + 3], head[at + 4]]) as usize;
                let w_px = u16::from_be_bytes([head[at + 5], head[at + 6]]) as usize;
                return if w_px > 0 && h_px > 0 { Some((w_px, h_px)) } else { None };
            }
            if len < 2 {
                return None;
            }
            at += len;
        }
    }
    None
}

/// Run the grabber and hand back the bytes.
///
/// Its own runner rather than `ExternalTool::run`, which returns a `String`.
/// A screen is ten megabytes of arbitrary bytes and roughly none of it is
/// valid text — putting it through a string conversion replaces every byte it
/// cannot make sense of with a replacement character, which for a picture is
/// most of them. It would not fail. It would hand back a picture of the right
/// length made mostly of the same three bytes over and over, and the finder
/// would report a blank screen.
pub fn pixels_from(tool: &crate::tools::ExternalTool, vars: &crate::tools::Vars, extra: &[String]) -> Result<Vec<u8>> {
    use std::io::Read;
    let (cmd, mut args) = tool.resolved(vars);
    args.extend(extra.iter().cloned());
    let mut child = crate::tools::command(&cmd)
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| AtlasError::Platform(format!("couldn't start {cmd}: {e}")))?;
    crate::childjob::tie(&child);
    let mut bytes = Vec::new();
    if let Some(mut out) = child.stdout.take() {
        out.read_to_end(&mut bytes)
            .map_err(|e| AtlasError::Platform(format!("couldn't read the picture: {e}")))?;
    }
    let done = child
        .wait_with_output()
        .map_err(|e| AtlasError::Platform(format!("{cmd} didn't finish: {e}")))?;
    if !done.status.success() {
        return Err(AtlasError::Platform(format!(
            "{cmd} failed: {}",
            String::from_utf8_lossy(&done.stderr).trim()
        )));
    }
    Ok(bytes)
}

/// Read the words in a picture file.
///
/// The size comes from the file's own header, and the pixels come from
/// `ffmpeg` — which is already how Atlas records, plays and screenshots, so
/// this adds no tool that was not there before. What it removes is the
/// tesseract install, which was never there at all.
pub fn read_file(
    reader: &mut Reader,
    ffmpeg: &crate::tools::ExternalTool,
    vars: &crate::tools::Vars,
    path: &str,
    cfg: &WordsConfig,
) -> Result<Screenful> {
    let head = head_of(path, 1024)?;
    let Some((w, h)) = picture_size(&head) else {
        return Err(AtlasError::Platform(format!(
            "I can't tell how big {path} is — I read the size out of a PNG or JPEG header and \
             that file is neither"
        )));
    };
    let bytes = pixels_from(ffmpeg, vars, &picture_args(path))?;
    whole_picture(bytes.len(), w, h)?;
    reader.look(&bytes, w, h, cfg)
}

/// The first few bytes of a file, without reading the rest of it.
///
/// A screenshot is ten megabytes and the size is in the first two dozen bytes.
fn head_of(path: &str, most: usize) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)
        .map_err(|e| AtlasError::Platform(format!("couldn't open {path}: {e}")))?;
    let mut buf = vec![0u8; most];
    let got = f
        .read(&mut buf)
        .map_err(|e| AtlasError::Platform(format!("couldn't read {path}: {e}")))?;
    buf.truncate(got);
    Ok(buf)
}

/// Check what came back is a whole picture of the size expected.
///
/// Separate from running `ffmpeg` so it can be tested without one.
pub fn whole_picture(got: usize, w: usize, h: usize) -> Result<()> {
    let want = w * h * 3;
    if got == want {
        return Ok(());
    }
    Err(AtlasError::Platform(format!(
        "the picture came back {} bytes and a {w}×{h} one is {want} — {}",
        got,
        if got < want {
            "reading part of a screen and not saying so is how Atlas would quote half a sentence \
             as the whole of it"
        } else {
            "that is more than fits, so it isn't the picture I asked for"
        }
    )))
}

/// A fractional box as whole pixels of a frame, or nothing if it is empty.
/// The words in one found line, as pixel boxes (x, y, width, height) of the
/// original picture, left to right.
///
/// A gap is a run of columns with no ink in them at least a third of the
/// line's height wide — wider than the space between letters, narrower than
/// the space between words at every size text is set in. Ink is anything
/// far enough from the line's own background, taken as its commonest
/// brightness, so dark-on-light and light-on-dark both work. A line with no
/// gap comes back whole.
pub fn words_in(rgb: &[u8], w: usize, h: usize, line: (usize, usize, usize, usize)) -> Vec<(usize, usize, usize, usize)> {
    let (lx, ly, lw, lh) = line;
    if lw == 0 || lh == 0 || rgb.len() < w * h * 3 {
        return vec![line];
    }
    let bright = |x: usize, y: usize| -> u8 {
        let i = (y * w + x) * 3;
        ((rgb[i] as u32 * 3 + rgb[i + 1] as u32 * 6 + rgb[i + 2] as u32) / 10) as u8
    };
    let mut counts = [0u32; 256];
    for y in ly..(ly + lh).min(h) {
        for x in lx..(lx + lw).min(w) {
            counts[bright(x, y) as usize] += 1;
        }
    }
    let background = counts.iter().enumerate().max_by_key(|(_, c)| **c).map(|(v, _)| v as i32).unwrap_or(255);
    let inked: Vec<bool> = (lx..(lx + lw).min(w))
        .map(|x| (ly..(ly + lh).min(h)).any(|y| (bright(x, y) as i32 - background).abs() > 60))
        .collect();
    let gap = (lh / 3).max(3);
    let mut words = Vec::new();
    let mut start: Option<usize> = None;
    let mut blank = 0usize;
    for (i, ink) in inked.iter().enumerate() {
        if *ink {
            if start.is_none() {
                start = Some(i);
            }
            blank = 0;
        } else if let Some(s0) = start {
            blank += 1;
            if blank >= gap {
                words.push((s0, i + 1 - blank));
                start = None;
                blank = 0;
            }
        }
    }
    if let Some(s0) = start {
        words.push((s0, inked.len() - blank));
    }
    if words.len() <= 1 {
        return vec![line];
    }
    // A little room either side of each word: the reader was trained on
    // crops with a margin, not letters touching the edge.
    let pad = (lh / 6).max(1);
    words
        .into_iter()
        .map(|(a, b)| {
            let x0 = (lx + a).saturating_sub(pad).max(lx);
            let x1 = (lx + b + pad).min(lx + lw);
            (x0, ly, x1 - x0, lh)
        })
        .collect()
}

pub fn crop_in(at: &Patch, w: usize, h: usize) -> Option<(usize, usize, usize, usize)> {
    if w == 0 || h == 0 {
        return None;
    }
    let x = (at.x.max(0.0) * w as f32).floor() as usize;
    let y = (at.y.max(0.0) * h as f32).floor() as usize;
    let right = (((at.x + at.width) * w as f32).ceil() as usize).min(w);
    let bottom = (((at.y + at.height) * h as f32).ceil() as usize).min(h);
    if right <= x || bottom <= y {
        return None;
    }
    Some((x, y, right - x, bottom - y))
}
