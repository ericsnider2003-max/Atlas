//! Editing photos, by voice or typing: always on a new copy beside the original.
//!
//! "Make this photo brighter", "crop it for Instagram", "straighten this",
//! "blur the background", "remove the background", "resize these for a
//! YouTube thumbnail", "fix all the photos in this folder".
//!
//! ## The engine is ffmpeg
//!
//! Atlas already fetches ffmpeg (`getpieces`), which decodes JPEG, PNG, WebP
//! and -- from 8.1 -- the tiled HEIC an iPhone writes, and has every filter
//! this needs: `transpose`/`rotate`/`crop`/`scale` for shape, `eq` and
//! `colorchannelmixer` for exposure and colour, `nlmeans`/`hqdn3d` to
//! denoise, `cas` to sharpen, `gblur` + `maskedmerge` and `alphamerge` for
//! backgrounds. No image crates: it runs as its own program, so its licence
//! stays with it. The one part that is Atlas's own is *measuring*: a small
//! copy of the photo comes back from ffmpeg as raw pixels and is read here,
//! so every change is stated as a number ("average brightness 31% -> 42%")
//! rather than a taste -- `grade`'s rule for video, applied to stills.
//!
//! ## Never overwrite
//!
//! Every edit writes a new file beside the original -- `trip.jpg` becomes
//! `trip.edited.jpg`, then `trip.edited-2.jpg` if that name is taken -- and
//! ffmpeg is also told never to overwrite (`-n`), so a race can't either.
//! "Undo" deletes the copies the last edit made, and says where the
//! original is. The original is only ever read.
//!
//! ## Which way up
//!
//! ffmpeg turns a photo the way its EXIF orientation tag (JPEG) or `irot`
//! property (HEIC) says before any filter sees it -- checked on 29 Sep 2026
//! with ffmpeg 6.1 and 9.0 (`tests/photo_editing.rs` makes a JPEG tagged
//! "turn 90°" and checks the copy comes out upright). The copy is written
//! with no metadata (`-map_metadata -1`): upright pixels and a stale tag
//! would turn it twice, and it also leaves the location out of a photo
//! you're about to post.
//!
//! ## Where the sizes come from
//!
//! One table, `PRESETS`. YouTube's from its own help page (answer 72431,
//! read 29 Sep 2026: 3840x2160 for videos, 2160x3840 for Shorts, 2 MB
//! from a phone, 50 MB from a computer); Instagram's from Buffer's March
//! 2026 guide (read the same day). TikTok, X, LinkedIn and Facebook are from
//! secondary guides only and marked `checked: false`, which is said.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// What counts as a photo, by extension.
pub const PHOTO_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp", "heic", "heif", "bmp", "tif", "tiff"];

/// ffmpeg reads iPhone photos whole from this version on.
pub const HEIC_FROM: (u32, u32) = (8, 1);

// ------------------------------------------------------------------ sizes

/// A place a photo is going, and the size it wants there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Preset {
    pub name: &'static str,
    pub width: u32,
    pub height: u32,
    /// Words that name it; the longest match wins.
    pub words: &'static [&'static str],
    /// Checked against the platform's own page (or Buffer's current guide
    /// for Instagram), rather than taken from a secondary guide.
    pub checked: bool,
    /// The largest file the platform takes everywhere, if it says.
    pub max_bytes: Option<u64>,
}

/// Every size Atlas knows, in one place.
pub const PRESETS: &[Preset] = &[
    Preset { name: "Instagram portrait", width: 1080, height: 1350, words: &["instagram", "insta", "instagram portrait", "instagram post", "instagram feed", "for the gram"], checked: true, max_bytes: None },
    Preset { name: "Instagram square", width: 1080, height: 1080, words: &["instagram square", "square"], checked: true, max_bytes: None },
    Preset { name: "Instagram 3:4", width: 1080, height: 1440, words: &["instagram 3:4", "instagram grid", "3:4"], checked: true, max_bytes: None },
    Preset { name: "Instagram landscape", width: 1080, height: 566, words: &["instagram landscape", "instagram wide"], checked: true, max_bytes: None },
    Preset { name: "Instagram story", width: 1080, height: 1920, words: &["instagram story", "story", "reel", "instagram reel"], checked: true, max_bytes: None },
    // 2 MB is YouTube's limit from a phone (50 MB from a computer); kept
    // under it so the thumbnail can be set from either.
    Preset { name: "YouTube thumbnail", width: 3840, height: 2160, words: &["youtube", "youtube thumbnail", "thumbnail", "yt thumbnail"], checked: true, max_bytes: Some(2_000_000) },
    Preset { name: "YouTube Shorts thumbnail", width: 2160, height: 3840, words: &["shorts", "youtube shorts", "shorts thumbnail"], checked: true, max_bytes: Some(2_000_000) },
    Preset { name: "TikTok photo", width: 1080, height: 1920, words: &["tiktok", "tik tok"], checked: false, max_bytes: None },
    Preset { name: "X post", width: 1600, height: 900, words: &["twitter", "for x", "x post", "tweet"], checked: false, max_bytes: None },
    Preset { name: "LinkedIn post", width: 1200, height: 627, words: &["linkedin"], checked: false, max_bytes: None },
    Preset { name: "Facebook link image", width: 1200, height: 630, words: &["facebook"], checked: false, max_bytes: None },
];

/// The size named in what was said, if any (the longest name wins, so
/// "instagram story" is not taken for "instagram").
fn preset_in(said: &str) -> Option<&'static Preset> {
    let l = format!(" {} ", said.to_lowercase());
    PRESETS
        .iter()
        .flat_map(|p| p.words.iter().map(move |w| (p, *w)))
        .filter(|(_, w)| l.contains(&format!(" {w} ")) || l.contains(&format!(" {w},")) || l.contains(&format!(" {w}.")) || l.contains(&format!(" {w}?")) || l.contains(&format!(" {w}'")))
        .max_by_key(|(_, w)| w.len())
        .map(|(p, _)| p)
}

// ------------------------------------------------------------------ what was asked

/// One change.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// Brighter (`true`) or darker.
    Lighter(bool),
    /// Measured exposure, contrast and colour cast, fixed together.
    AutoFix,
    /// More contrast (`true`) or less.
    Contrast(bool),
    /// Warmer (`true`) or cooler.
    Warmth(bool),
    /// Black and white.
    Grey,
    Denoise,
    Sharpen,
    /// A quarter turn or more, clockwise: 90, 180 or 270.
    Turn(u16),
    /// Left to right.
    Mirror,
    /// Turn this many degrees clockwise and crop the empty corners away.
    Tilt(f32),
    /// Measure how far off level it is and offer to fix it.
    Straighten,
    /// "Yes, straighten it": the offer made last time.
    StraightenAccepted,
    /// Crop to the shape and scale to the size.
    Fit(Preset),
    /// A plain size, keeping the shape: "1920 wide", "half size".
    Resize(Size),
    BlurBackground,
    RemoveBackground,
}

impl Op {
    /// Where in the chain it goes: shape first, then light and colour, then
    /// noise, then the background, then the final size, then sharpening
    /// (after the resize, never before).
    fn chain_place(&self) -> u8 {
        match self {
            Op::Turn(_) | Op::Mirror | Op::Tilt(_) | Op::Straighten | Op::StraightenAccepted => 0,
            Op::AutoFix | Op::Lighter(_) | Op::Contrast(_) | Op::Warmth(_) | Op::Grey => 1,
            Op::Denoise => 2,
            Op::BlurBackground | Op::RemoveBackground => 3,
            Op::Fit(_) | Op::Resize(_) => 4,
            Op::Sharpen => 5,
        }
    }
}

/// A size asked for in words, the shape kept.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Size {
    Wide(u32),
    Tall(u32),
    /// Fit inside this box: "1920x1080".
    Within(u32, u32),
    /// A share of the size: 0.5 for "half size" or "50%".
    Scale(f32),
}

impl Size {
    /// The size a `w` x `h` picture becomes, never below 16 pixels a side.
    pub fn of(self, w: u32, h: u32) -> (u32, u32) {
        let (fw, fh) = (w.max(1) as f64, h.max(1) as f64);
        let k = match self {
            Size::Wide(x) => x as f64 / fw,
            Size::Tall(y) => y as f64 / fh,
            Size::Within(x, y) => (x as f64 / fw).min(y as f64 / fh),
            Size::Scale(k) => k as f64,
        };
        (((fw * k).round() as u32).max(16), ((fh * k).round() as u32).max(16))
    }
}

/// "1920 wide", "2000 pixels tall", "1920x1080", "half size", "50%",
/// "quarter size" -- only with a sizing word, so "rotate 90" and "the 3
/// photos" aren't sizes.
fn size_in(l: &str) -> Option<Size> {
    if !has(l, &[" resize", " smaller", " shrink", " scale", " downsize", " size", " wide", " tall", "px"]) {
        return None;
    }
    if has(l, &[" half size", " half the size", " by half", " halve"]) {
        return Some(Size::Scale(0.5));
    }
    if has(l, &[" quarter size", " quarter the size", " a quarter"]) {
        return Some(Size::Scale(0.25));
    }
    let words: Vec<&str> = l.split_whitespace().collect();
    for (i, word) in words.iter().enumerate() {
        let t = word.trim_end_matches([',', '.', '?', '!']);
        if let Some(pc) = t.strip_suffix('%') {
            if let Ok(v) = pc.parse::<f32>() {
                if v > 0.0 && v < 100.0 {
                    return Some(Size::Scale(v / 100.0));
                }
            }
            continue;
        }
        if let Some((a, b)) = t.split_once(['x', '\u{d7}']) {
            if let (Ok(x), Ok(y)) = (a.parse::<u32>(), b.trim_end_matches("px").parse::<u32>()) {
                if (16..=20000).contains(&x) && (16..=20000).contains(&y) {
                    return Some(Size::Within(x, y));
                }
            }
        }
        let Ok(n) = t.trim_end_matches("px").parse::<u32>() else { continue };
        if !(16..=20000).contains(&n) {
            continue;
        }
        let after: Vec<&str> = words[i + 1..].iter().take(2).copied().collect();
        let next = |w: &str| after.iter().any(|a| a.starts_with(w));
        if next("wide") || next("across") || next("width") {
            return Some(Size::Wide(n));
        }
        if next("tall") || next("high") || next("height") {
            return Some(Size::Tall(n));
        }
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Format {
    Jpeg,
    Png,
    Webp,
}

impl Format {
    pub fn ext(self) -> &'static str {
        match self {
            Format::Jpeg => "jpg",
            Format::Png => "png",
            Format::Webp => "webp",
        }
    }

    fn of(path: &Path) -> Format {
        match ext_of(path).as_str() {
            "png" | "tif" | "tiff" => Format::Png,
            "webp" => Format::Webp,
            _ => Format::Jpeg,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Format::Jpeg => "JPEG",
            Format::Png => "PNG",
            Format::Webp => "WebP",
        }
    }
}

/// What was asked for, read from the words.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Wish {
    pub ops: Vec<Op>,
    pub format: Option<Format>,
    /// Every photo in a folder, not one.
    pub folder: bool,
    /// Take the last edit back.
    pub undo: bool,
    /// Fetch the cut-out models.
    pub get_models: bool,
}

fn has(l: &str, any: &[&str]) -> bool {
    any.iter().any(|w| l.contains(w))
}

/// Read what's wanted from the words (with any path already taken out).
pub fn read_wish(said: &str) -> Wish {
    let l = format!(" {} ", said.to_lowercase().replace(['’', '‘'], "'"));
    let mut w = Wish::default();
    if has(&l, &["photo models", "cut-out models", "cutout models", "cut out models", "background models"]) && has(&l, &["get", "fetch", "download", "install"]) {
        w.get_models = true;
        return w;
    }
    if has(&l, &[" undo", "take back", "put the photo back", "delete the edited", "delete the copy", "get rid of the edit"]) {
        w.undo = true;
        return w;
    }
    w.folder = has(&l, &[" all the photos", " all photos", " all my photos", " every photo", " these", " folder", " all of them", " the lot", " batch"]);

    if has(&l, &[" brighter", " brighten", " lighten", " lighter", " too dark", " more light", " lift the shadows"]) {
        w.ops.push(Op::Lighter(true));
    } else if has(&l, &[" darker", " darken", " too bright", " less bright", " overexposed"]) {
        w.ops.push(Op::Lighter(false));
    }
    if has(&l, &[" fix ", " fix it", " fix the photo", " enhance", " auto ", " improve", " look better", " white balance", " colour cast", " color cast", " fix the colo", " exposure", " correct the colo", " touch up", " touch it up"]) {
        w.ops.push(Op::AutoFix);
    }
    if has(&l, &[" more contrast", " punchier", " more punch", " add contrast", " increase contrast", " boost contrast"]) {
        w.ops.push(Op::Contrast(true));
    } else if has(&l, &[" less contrast", " softer", " flatter", " reduce contrast", " lower contrast"]) {
        w.ops.push(Op::Contrast(false));
    }
    if has(&l, &[" warmer", " warm it", " warm up", " less blue"]) {
        w.ops.push(Op::Warmth(true));
    } else if has(&l, &[" cooler", " cool it", " cool down", " less yellow", " less orange"]) {
        w.ops.push(Op::Warmth(false));
    }
    if has(&l, &[" black and white", " black & white", " b&w", " grayscale", " greyscale", " monochrome", " mono "]) {
        w.ops.push(Op::Grey);
    }
    if has(&l, &[" denoise", " noise", " grain", " noisy", " grainy"]) {
        w.ops.push(Op::Denoise);
    }
    if has(&l, &[" sharpen", " sharper", " crisper", " crisp", " blurry", " soft focus"]) && !has(&l, &[" blur the background", " background blur"]) {
        w.ops.push(Op::Sharpen);
    }
    if has(&l, &[" remove the background", " remove background", " remove the bg", " cut out", " cutout", " transparent background", " no background", " without the background", " background removed", " take out the background", " take the background out", " lose the background"]) {
        w.ops.push(Op::RemoveBackground);
    } else if has(&l, &[" blur the background", " background blur", " blurred background", " blur background", " blur behind", " bokeh", " portrait mode", " blur out the background", " soften the background"]) {
        w.ops.push(Op::BlurBackground);
    }

    // Turning. A number of degrees says how far; 90/180/270 are quarter turns.
    let degrees = degrees_in(&l);
    let anticlockwise = has(&l, &[" anticlockwise", " anti-clockwise", " counterclockwise", " counter-clockwise", " counter clockwise", " to the left", " left"]);
    let yes = has(&l, &[" yes", " go ahead", " do it", " please do", " sure", " ok ", " okay"]);
    if has(&l, &[" straighten", " level it", " level the", " horizon", " crooked", " wonky", " tilted", " not straight", " isn't straight", " is it level", " photo level", " is this level"]) {
        match degrees {
            Some(d) => w.ops.push(Op::Tilt(if anticlockwise { -d } else { d })),
            None if yes => w.ops.push(Op::StraightenAccepted),
            None => w.ops.push(Op::Straighten),
        }
    } else if has(&l, &[" rotate", " turn it", " turn the photo", " turn this", " sideways", " upside down", " tilt"]) {
        match degrees {
            Some(d) if (d - 90.0).abs() < 0.01 => w.ops.push(Op::Turn(if anticlockwise { 270 } else { 90 })),
            Some(d) if (d - 180.0).abs() < 0.01 => w.ops.push(Op::Turn(180)),
            Some(d) if (d - 270.0).abs() < 0.01 => w.ops.push(Op::Turn(if anticlockwise { 90 } else { 270 })),
            Some(d) => w.ops.push(Op::Tilt(if anticlockwise { -d } else { d })),
            None if l.contains("upside down") => w.ops.push(Op::Turn(180)),
            None => w.ops.push(Op::Turn(if anticlockwise { 270 } else { 90 })),
        }
    }
    if has(&l, &[" flip", " mirror"]) {
        w.ops.push(Op::Mirror);
    }
    if let Some(p) = preset_in(&l) {
        w.ops.push(Op::Fit(*p));
    } else if let Some(size) = size_in(&l) {
        w.ops.push(Op::Resize(size));
    }

    w.format = if has(&l, &[" png", " as a png", " transparent"]) {
        Some(Format::Png)
    } else if has(&l, &[" webp"]) {
        Some(Format::Webp)
    } else if has(&l, &[" jpg", " jpeg"]) {
        Some(Format::Jpeg)
    } else {
        None
    };
    // "Resize these" with no size named is a question, not an edit.
    w.ops.sort_by_key(Op::chain_place);
    w.ops.dedup();
    w
}

/// "2.5 degrees", "3°", "by 90" -- the first number before a degree word,
/// or after "rotate"/"by".
fn degrees_in(l: &str) -> Option<f32> {
    let words: Vec<&str> = l.split_whitespace().collect();
    for (i, word) in words.iter().enumerate() {
        let t = word.trim_end_matches(['°', ',', '.', '?']);
        let Ok(n) = t.trim_start_matches('+').parse::<f32>() else { continue };
        let next = words.get(i + 1).copied().unwrap_or("");
        let prev = if i > 0 { words[i - 1] } else { "" };
        if word.ends_with('°') || next.starts_with("degree") || next.starts_with("deg") || matches!(prev, "by" | "rotate" | "turn") {
            if n.is_finite() && n.abs() > 0.0 && n.abs() <= 360.0 {
                return Some(n);
            }
        }
    }
    None
}

// ------------------------------------------------------------------ measuring

/// A picture as raw RGB.
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

/// A photo as ffmpeg sees it once it's the right way up: its full size, and
/// a small copy to measure.
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    pub width: u32,
    pub height: u32,
    pub small: Picture,
}

/// Light and colour, measured on the small copy. All 0..1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stats {
    pub mean: f32,
    pub p1: f32,
    pub p99: f32,
    /// Share of pixels at the very bottom and top.
    pub crushed: f32,
    pub clipped: f32,
    /// Mean red, green, blue.
    pub channel: [f32; 3],
    /// How many pixels at each brightness, 0..=255.
    pub hist: [u32; 256],
}

pub fn stats(p: &Picture) -> Stats {
    let n = (p.rgb.len() / 3).max(1);
    let mut hist = [0u32; 256];
    let mut sum = [0f64; 3];
    for px in p.rgb.chunks_exact(3) {
        let y = (0.299 * px[0] as f32 + 0.587 * px[1] as f32 + 0.114 * px[2] as f32).round() as usize;
        hist[y.min(255)] += 1;
        for c in 0..3 {
            sum[c] += px[c] as f64;
        }
    }
    let pct = |q: f32| {
        let want = (q * n as f32).ceil() as u32;
        let mut seen = 0;
        for (v, &c) in hist.iter().enumerate() {
            seen += c;
            if seen >= want.max(1) {
                return v as f32 / 255.0;
            }
        }
        1.0
    };
    let mean = hist.iter().enumerate().map(|(v, &c)| v as f64 * c as f64).sum::<f64>() / n as f64 / 255.0;
    Stats {
        mean: mean as f32,
        p1: pct(0.01),
        p99: pct(0.99),
        crushed: hist[..=3].iter().sum::<u32>() as f32 / n as f32,
        clipped: hist[252..].iter().sum::<u32>() as f32 / n as f32,
        channel: [0, 1, 2].map(|c| (sum[c] / n as f64 / 255.0) as f32),
        hist,
    }
}

/// Read a binary PPM (P6) or PGM (P5), as ffmpeg writes them.
fn read_pnm(bytes: &[u8]) -> Option<Picture> {
    let mut fields = Vec::new();
    let mut i = 0;
    while fields.len() < 4 && i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'#' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        let s = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        fields.push(std::str::from_utf8(&bytes[s..i]).ok()?.to_string());
    }
    let channels = match fields.first()?.as_str() {
        "P6" => 3,
        "P5" => 1,
        _ => return None,
    };
    let (w, h, max): (u32, u32, u32) = (fields.get(1)?.parse().ok()?, fields.get(2)?.parse().ok()?, fields.get(3)?.parse().ok()?);
    if max != 255 {
        return None;
    }
    let data = bytes.get(i + 1..)?;
    let want = w as usize * h as usize * channels;
    if data.len() < want {
        return None;
    }
    let rgb = if channels == 3 { data[..want].to_vec() } else { data[..want].iter().flat_map(|&g| [g, g, g]).collect() };
    Some(Picture { width: w, height: h, rgb })
}

/// Run ffmpeg with a time limit, keeping what it printed. A photo is seconds
/// of work; anything past `limit` is an ffmpeg stuck on something, killed.
fn run(ffmpeg: &str, args: &[String], limit: std::time::Duration) -> Result<std::process::Output, String> {
    use std::io::Read;
    use std::process::Stdio;
    let mut child = crate::tools::command(ffmpeg)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("I couldn't start ffmpeg ({e})"))?;
    let mut so = child.stdout.take();
    let mut se = child.stderr.take();
    let out_t = std::thread::spawn(move || {
        let mut b = Vec::new();
        if let Some(p) = so.as_mut() {
            crate::heard!(p.read_to_end(&mut b));
        }
        b
    });
    let err_t = std::thread::spawn(move || {
        let mut b = Vec::new();
        if let Some(p) = se.as_mut() {
            crate::heard!(p.read_to_end(&mut b));
        }
        b
    });
    let began = std::time::Instant::now();
    let mut polls = 0;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if began.elapsed() > limit => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("ffmpeg took longer than {} seconds, so I stopped it", limit.as_secs()));
            }
            Ok(None) => {
                std::thread::sleep(crate::tools::poll_gap(polls));
                polls += 1;
            }
            Err(e) => return Err(format!("I lost track of ffmpeg ({e})")),
        }
    };
    Ok(std::process::Output { status, stdout: out_t.join().unwrap_or_default(), stderr: err_t.join().unwrap_or_default() })
}

const LIMIT: std::time::Duration = std::time::Duration::from_secs(300);

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

/// The last thing ffmpeg complained about, for a sentence.
fn complaint(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    text.lines().rev().map(str::trim).find(|l| !l.is_empty()).unwrap_or("it gave no reason").chars().take(200).collect()
}

/// A small copy of the photo, the right way up, scaled to fit `fit` x `fit`
/// (or stretched to exactly `exact`), plus its full upright size.
fn small_copy(ffmpeg: &str, path: &Path, fit: u32, exact: Option<(u32, u32)>) -> Result<Look, String> {
    let scale = match exact {
        Some((w, h)) => format!("scale={w}:{h}:flags=area"),
        None => format!("scale={fit}:{fit}:force_original_aspect_ratio=decrease:flags=area"),
    };
    let mut args = s(&["-hide_banner", "-nostdin", "-i"]);
    args.push(path.display().to_string());
    // `-filter_complex` with its input left unnamed, never `-vf`: ffmpeg
    // (8.1 on) assembles an iPhone photo's tiles in a filter graph of its
    // own, and refuses a `-vf` on what comes out of one ("Simple and complex
    // filtering cannot be used together"), while `[0:v]` would be the first
    // tile alone. Unnamed, the graph takes the whole assembled picture --
    // and, for a JPEG, the same upright frame `-vf` would (29 Sep 2026,
    // ffmpeg 6.1 and 9.0).
    args.extend(s(&["-frames:v", "1", "-filter_complex"]));
    args.push(format!("showinfo,{scale},format=rgb24"));
    args.extend(s(&["-f", "image2pipe", "-c:v", "ppm", "-"]));
    let out = run(ffmpeg, &args, LIMIT)?;
    let small = read_pnm(&out.stdout).ok_or_else(|| format!("ffmpeg couldn't read it: {}", complaint(&out.stderr)))?;
    // showinfo prints the frame as it arrives, after ffmpeg's own turning:
    // "... s:4032x3024 ...".
    let text = String::from_utf8_lossy(&out.stderr);
    let (width, height) = text
        .split_whitespace()
        .filter_map(|t| t.strip_prefix("s:"))
        .find_map(|d| {
            let (a, b) = d.split_once('x')?;
            Some((a.parse().ok()?, b.parse().ok()?))
        })
        .unwrap_or((small.width, small.height));
    Ok(Look { width, height, small })
}

/// Measure a photo: its upright size, and a copy about 512 px across.
pub fn look_at_photo(ffmpeg: &str, path: &Path) -> Result<Look, String> {
    small_copy(ffmpeg, path, 512, None)
}

/// "ffmpeg version 9.0.2-essentials_build-www.gyan.dev", "n9.0.2-14-g..." or
/// "6.1.1-3ubuntu5" -> (9, 0) / (6, 1). A git build ("N-12345-g...") has no
/// number and is taken to be new.
pub fn ffmpeg_version(banner: &str) -> Option<(u32, u32)> {
    let v = banner.split("version").nth(1)?.split_whitespace().next()?;
    let v = v.trim_start_matches(['n', 'v']);
    if v.starts_with('N') {
        return Some((u32::MAX, 0));
    }
    let mut parts = v.split(|c: char| !c.is_ascii_digit());
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().and_then(|m| m.parse().ok()).unwrap_or(0);
    Some((major, minor))
}

fn ext_of(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn is_heic(path: &Path) -> bool {
    matches!(ext_of(path).as_str(), "heic" | "heif")
}

/// Is this ffmpeg new enough for iPhone photos? `Err` says so in words.
fn heic_ok(ffmpeg: &str) -> Result<(), String> {
    let out = run(ffmpeg, &s(&["-hide_banner", "-version"]), std::time::Duration::from_secs(20))?;
    let banner = String::from_utf8_lossy(&out.stdout).to_string();
    match ffmpeg_version(&banner) {
        Some(v) if v >= HEIC_FROM => Ok(()),
        Some((a, b)) => Err(format!(
            // 7.x hands back one 512-pixel tile; 6.x can't open one at all
            // (checked 29 Sep 2026 with 6.1.1 and a real tiled HEIC).
            "That's an iPhone photo (HEIC), and the ffmpeg here is version {a}.{b}, which can't read one whole -- \
             at best it gets the first 512-pixel tile. Atlas's setup now fetches 9.0.2 -- run setup again and I'll be able to."
        )),
        None => Ok(()),
    }
}

// ------------------------------------------------------------------ the changes, as filters

fn pct(v: f32) -> String {
    format!("{:.0}%", v * 100.0)
}

/// The measured fix for exposure, contrast and colour cast: filters, and
/// what each did in words.
fn auto_fix(st: &Stats) -> (Vec<String>, Vec<String>) {
    let mut f = Vec::new();
    let mut said = Vec::new();
    // Colour cast: grey-world gains, when a channel is more than 5% off the
    // green one. Numbers, not a filter's own guess.
    let [r, g, b] = st.channel;
    if g > 0.02 {
        let (kr, kb) = ((g / r.max(1e-3)).clamp(0.75, 1.33), (g / b.max(1e-3)).clamp(0.75, 1.33));
        if (kr - 1.0).abs() > 0.05 || (kb - 1.0).abs() > 0.05 {
            f.push(format!("colorchannelmixer=rr={kr:.3}:bb={kb:.3}"));
            let (cast, over) = if b > r { ("blue", b / g - 1.0) } else { ("warm orange", r / g - 1.0) };
            said.push(format!("took out a {cast} cast ({} was {} over green)", if b > r { "blue" } else { "red" }, pct(over.abs())));
        }
    }
    // Levels: stretch the 1st..99th percentile towards the full range,
    // unless they already nearly fill it.
    let span = (st.p99 - st.p1).max(0.05);
    let mut contrast = 1.0f32;
    let mut brightness = 0.0f32;
    if st.p1 > 0.03 || st.p99 < 0.95 {
        contrast = (0.94 / span).clamp(1.0, 1.6);
        let low = (st.p1 - 0.5) * contrast + 0.5;
        brightness = (0.02 - low).clamp(-0.25, 0.25);
    }
    // Exposure: the middle towards 45% with gamma, after the levels.
    let mid = ((st.mean - 0.5) * contrast + 0.5 + brightness).clamp(0.02, 0.98);
    let mut gamma = 1.0f32;
    if !(0.38..=0.62).contains(&mid) {
        gamma = (mid.ln() / 0.45f32.ln()).clamp(0.7, 1.6);
    }
    if contrast > 1.01 || brightness.abs() > 0.005 || (gamma - 1.0).abs() > 0.01 {
        f.push(format!("eq=contrast={contrast:.3}:brightness={brightness:.3}:gamma={gamma:.3}"));
        if contrast > 1.01 {
            said.push(format!("stretched the contrast (the tones filled {} of the range)", pct(span)));
        }
        said.push(format!("average brightness {} -> {AFTER}", pct(st.mean)));
    }
    if st.clipped > 0.02 {
        said.push(format!("{} of it was already pure white, which can't be brought back", pct(st.clipped)));
    }
    if f.is_empty() {
        said.push("it already measured well-exposed with no colour cast, so the light and colour are unchanged".into());
    }
    (f, said)
}

/// The average brightness `gamma` would give, worked out from the histogram
/// (the average of the curve, not the curve of the average: a photo with a
/// lot of black in it moves less than its average suggests).
fn mean_after(st: &Stats, gamma: f32) -> f32 {
    let n: u32 = st.hist.iter().sum();
    let sum: f32 = st.hist.iter().enumerate().map(|(v, &c)| c as f32 * (v as f32 / 255.0).powf(1.0 / gamma)).sum();
    sum / n.max(1) as f32
}

/// The gamma that brings the average to `target`, found by halving (the
/// average rises steadily with gamma), within 0.4..2.5.
fn gamma_for(st: &Stats, target: f32) -> f32 {
    let (mut lo, mut hi) = (0.4f32, 2.5f32);
    for _ in 0..30 {
        let mid = (lo + hi) / 2.0;
        if mean_after(st, mid) < target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo + hi) / 2.0
}

/// Stands for the average brightness of the finished copy in a sentence,
/// filled in once the copy has been measured.
const AFTER: &str = "{after}";

/// Brighter or darker by lifting or lowering the middle tones: the average
/// moved by about 12 points, with the gamma worked out from the histogram.
fn lighter(st: &Stats, up: bool) -> (String, String) {
    let m = st.mean.clamp(0.02, 0.98);
    let target = if up { (m + 0.12).min(0.8).max(m + 0.04) } else { (m - 0.12).max(0.1).min(m - 0.04) };
    let gamma = gamma_for(st, target);
    (format!("eq=gamma={gamma:.3}"), format!("{} (average brightness {} -> {AFTER})", if up { "brighter" } else { "darker" }, pct(st.mean)))
}

/// Where to crop a `w` x `h` picture for `p` and then scale, as filters.
fn fit(p: &Preset) -> String {
    let (w, h) = (p.width, p.height);
    format!("crop='min(iw,ih*{w}/{h})':'min(ih,iw*{h}/{w})',scale={w}:{h}:flags=lanczos,setsar=1")
}

/// The size after a quarter turn.
fn turned(w: u32, h: u32, t: u16) -> (u32, u32) {
    if t == 90 || t == 270 { (h, w) } else { (w, h) }
}

// ------------------------------------------------------------------ memory

/// What the last edit made, and any straightening offered: kept in the store
/// so "undo" and "yes, straighten it" work after a restart too.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Memory {
    /// (original, copy) for every file the last edit made.
    #[serde(default)]
    pub made: Vec<(String, String)>,
    /// The photo a straightening was offered for, and by how much.
    #[serde(default)]
    pub offer: Option<(String, f32)>,
    /// The photo last worked on and what came of it: "now crop it" carries on
    /// from the copy.
    #[serde(default)]
    pub last: Option<(String, String)>,
    /// The words of the last edit that changed something (paths taken out),
    /// so "do the same to the whole folder" knows what "the same" is.
    #[serde(default)]
    pub last_ask: Option<String>,
}

pub const MEMORY: &str = "photo_edits";

impl Memory {
    pub fn load(state: &Path) -> Memory {
        crate::store::Store::new(state).load(MEMORY)
    }

    pub fn save(&self, state: &Path) {
        crate::kept!(crate::store::Store::new(state).save(MEMORY, self));
    }
}

// ------------------------------------------------------------------ doing it

/// Where things are.
#[derive(Debug, Clone)]
pub struct Setup {
    /// The ffmpeg program (a name on the PATH or a full path).
    pub ffmpeg: String,
    /// Where `cutout`'s models are (`models/`).
    pub models: PathBuf,
    /// The store's folder, for `Memory`.
    pub state: PathBuf,
    /// A folder for the in-between files (masks); emptied as it goes.
    pub scratch: PathBuf,
    /// The install, where `getpieces` puts the models.
    pub install: PathBuf,
}

impl Setup {
    /// This install: its models, its scratch folder, the store at `state`.
    pub fn here(ffmpeg: &str, state: &Path) -> Setup {
        Setup {
            ffmpeg: ffmpeg.to_string(),
            models: crate::roots::models_dir(),
            state: state.to_path_buf(),
            scratch: crate::roots::tmp_dir().join("photo"),
            install: crate::roots::install_root(),
        }
    }
}

/// What came of one photo.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// A new file, beside the original.
    Made { original: PathBuf, copy: PathBuf, said: String },
    /// Measured, and a straightening offered (nothing written).
    Offered { photo: PathBuf, fix: f32, said: String },
    /// Nothing written; this says why.
    Said(String),
}

/// A free name beside `original`: `name.edited.ext`, then `name.edited-2.ext`
/// and so on. An original that is itself an edited copy numbers on from it
/// rather than piling up `.edited.edited`.
pub fn copy_path(original: &Path, format: Format) -> PathBuf {
    let dir = original.parent().map(Path::to_path_buf).unwrap_or_default();
    let stem = original.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "photo".into());
    let base = match stem.rfind(".edited") {
        Some(i) if stem[i + 7..].is_empty() || stem[i + 7..].strip_prefix('-').is_some_and(|n| n.chars().all(|c| c.is_ascii_digit())) => stem[..i].to_string(),
        _ => stem,
    };
    let ext = format.ext();
    let first = dir.join(format!("{base}.edited.{ext}"));
    if !first.exists() {
        return first;
    }
    let mut n = 2u64;
    loop {
        let p = dir.join(format!("{base}.edited-{n}.{ext}"));
        if !p.exists() {
            return p;
        }
        n += 1;
    }
}

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.display().to_string())
}

/// Encoder arguments for a format. `q` is JPEG's quality step (2 best, 31 worst).
fn encode(format: Format, q: u32) -> Vec<String> {
    match format {
        Format::Jpeg => vec!["-c:v".into(), "mjpeg".into(), "-q:v".into(), q.to_string()],
        Format::Png => s(&["-c:v", "png"]),
        Format::Webp => s(&["-c:v", "libwebp", "-quality", "90"]),
    }
}

/// Write the matte where ffmpeg can read it: a PGM.
fn write_mask(m: &crate::cutout::Matte, path: &Path) -> Result<(), String> {
    let mut b = format!("P5\n{} {}\n255\n", m.width, m.height).into_bytes();
    b.extend_from_slice(&m.alpha);
    std::fs::write(path, b).map_err(|e| format!("I couldn't write the cut-out to a scratch file ({e})"))
}

/// A scratch file name that two errands can't share.
fn scratch_file(dir: &Path, what: &str, ext: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    dir.join(format!("photo-{}-{n}-{what}.{ext}", std::process::id()))
}

/// Do `wish` to one photo. `batch` picks the quicker denoiser.
pub fn edit_one(setup: &Setup, photo: &Path, wish: &Wish, offer: Option<f32>, batch: bool) -> Outcome {
    match edit_inner(setup, photo, wish, offer, batch) {
        Ok(o) => o,
        Err(why) => Outcome::Said(format!("I couldn't edit {}: {why}. The original is untouched.", name_of(photo))),
    }
}

fn edit_inner(setup: &Setup, photo: &Path, wish: &Wish, offer: Option<f32>, batch: bool) -> Result<Outcome, String> {
    let ffmpeg = setup.ffmpeg.as_str();
    if !photo.is_file() {
        return Ok(Outcome::Said(format!("I can't find {}.", photo.display())));
    }
    if is_heic(photo) {
        if let Err(why) = heic_ok(ffmpeg) {
            return Ok(Outcome::Said(why));
        }
    }
    let seen = look_at_photo(ffmpeg, photo)?;

    // Straightening on its own is a measurement and an offer, never an edit.
    if wish.ops.contains(&Op::Straighten) {
        let g = crate::straighten::grey(&seen.small.rgb);
        let said_name = name_of(photo);
        return Ok(match crate::straighten::measure(&g, seen.small.width as usize, seen.small.height as usize) {
            Some(t) if t.sure() && !t.level() => Outcome::Offered { photo: photo.to_path_buf(), fix: t.fix, said: crate::straighten::offer(&t, &said_name) },
            Some(t) => Outcome::Said(crate::straighten::offer(&t, &said_name)),
            None => Outcome::Said(format!("{said_name} is too plain for me to tell which way is level -- there are hardly any edges in it.")),
        });
    }

    let st = stats(&seen.small);
    let (mut w, mut h) = (seen.width, seen.height);
    let mut pre: Vec<String> = Vec::new();
    let mut post: Vec<String> = Vec::new();
    let mut said: Vec<String> = Vec::new();
    let mut background = None;
    let mut max_bytes = None;
    for op in &wish.ops {
        match op {
            Op::Turn(t) => {
                pre.push(match t {
                    90 => "transpose=clock".into(),
                    270 => "transpose=cclock".into(),
                    _ => "hflip,vflip".into(),
                });
                (w, h) = turned(w, h, *t);
                said.push(match t {
                    90 => "turned a quarter clockwise".to_string(),
                    270 => "turned a quarter anticlockwise".to_string(),
                    _ => "turned upside down".to_string(),
                });
            }
            Op::Mirror => {
                pre.push("hflip".into());
                said.push("flipped left to right".into());
            }
            Op::Tilt(_) | Op::StraightenAccepted => {
                let d = match op {
                    Op::Tilt(d) => *d,
                    _ => offer.ok_or("there's no straightening waiting to be done -- ask me to straighten it first")?,
                };
                let k = crate::straighten::crop_after_turning(w as f64, h as f64, d as f64);
                let (cw, ch) = (((w as f64 * k) as u32) & !1, ((h as f64 * k) as u32) & !1);
                pre.push(format!("rotate={:.6}:ow=iw:oh=ih:c=black,crop={cw}:{ch}", (d as f64).to_radians()));
                (w, h) = (cw, ch);
                said.push(format!("turned {:.1}° {} and trimmed the corners ({} of it kept)", d.abs(), if d >= 0.0 { "clockwise" } else { "anticlockwise" }, pct(k as f32)));
            }
            Op::AutoFix => {
                let (f, words) = auto_fix(&st);
                pre.extend(f);
                said.extend(words);
            }
            Op::Lighter(up) => {
                let (f, words) = lighter(&st, *up);
                pre.push(f);
                said.push(words);
            }
            Op::Contrast(up) => {
                pre.push(format!("eq=contrast={}", if *up { "1.15" } else { "0.87" }));
                said.push(if *up { "more contrast (x1.15)".into() } else { "less contrast (x0.87)".into() });
            }
            Op::Warmth(up) => {
                pre.push(if *up { "colorchannelmixer=rr=1.06:bb=0.94".into() } else { "colorchannelmixer=rr=0.94:bb=1.06".into() });
                said.push(if *up { "warmer (red up 6%, blue down 6%)".into() } else { "cooler (blue up 6%, red down 6%)".into() });
            }
            Op::Grey => {
                pre.push("hue=s=0".into());
                said.push("black and white".into());
            }
            Op::Denoise => {
                // nlmeans is the better one and about 6 s for 12 MP on two
                // cores; hqdn3d is under 1 s, for a folder.
                pre.push(if batch { "hqdn3d=4:3:6:4.5".into() } else { "nlmeans=s=2.5:p=5:r=9".into() });
                said.push("smoothed the noise".into());
            }
            // The matte is made at the size the picture has here, before
            // any final crop.
            Op::BlurBackground | Op::RemoveBackground => background = Some((op.clone(), w, h)),
            Op::Fit(p) => {
                post.push(fit(p));
                max_bytes = p.max_bytes;
                // The part kept by the crop, against the size it's scaled to.
                let kept = (w as f32).min(h as f32 * p.width as f32 / p.height as f32);
                let small = kept / p.width as f32;
                said.push(format!(
                    "cropped to {} ({}x{}){}{}",
                    p.name,
                    p.width,
                    p.height,
                    if small < 0.95 { format!(" -- it was only {w}x{h}, so it's been enlarged and may look soft") } else { String::new() },
                    if p.checked { "" } else { "; that size is from secondary guides, not the platform's own page" }
                ));
                (w, h) = (p.width, p.height);
            }
            Op::Resize(size) => {
                let (nw, nh) = size.of(w, h);
                post.push(format!("scale={nw}:{nh}:flags=lanczos,setsar=1"));
                said.push(format!(
                    "resized to {nw}x{nh}{}",
                    if nw > w { " -- that's larger than it was, so it may look soft" } else { "" }
                ));
                (w, h) = (nw, nh);
            }
            Op::Sharpen => {
                post.push("cas=strength=0.5".into());
                said.push("sharpened".into());
            }
            Op::Straighten => {}
        }
    }
    let mut format = wish.format.unwrap_or_else(|| Format::of(photo));
    if matches!(background, Some((Op::RemoveBackground, ..))) && format == Format::Jpeg {
        // A JPEG can't be see-through.
        format = Format::Png;
        said.push("saved as PNG, because a JPEG can't be see-through".into());
    } else if wish.format.is_some() && wish.format != Some(Format::of(photo)) {
        said.push(format!("saved as {}", format.name()));
    }
    if pre.is_empty() && post.is_empty() && background.is_none() && wish.format.is_none() {
        return Ok(Outcome::Said("I didn't catch what to change -- brighter, fix the colours, straighten, crop it for Instagram, blur the background...?".into()));
    }

    std::fs::create_dir_all(&setup.scratch).map_err(|e| format!("I couldn't make a scratch folder ({e})"))?;
    let mut scratch: Vec<PathBuf> = Vec::new();
    let result = (|| -> Result<Outcome, String> {
        let mut source = photo.to_path_buf();
        let mut graph: Vec<String> = pre.clone();
        let mut inputs = vec![source.display().to_string()];
        let complex;
        if let Some((bg, w, h)) = background.clone() {
            let Some((by, model)) = crate::cutout::installed(&setup.models) else {
                return Ok(Outcome::Said(crate::cutout::missing()));
            };
            // The shape and light changes first, to a scratch PNG, so the
            // matte is made on the picture it will be laid over. Always, even
            // with nothing to change: the graph below names its two inputs,
            // and `[0:v]` of an iPhone photo is its first tile, not the photo.
            {
                let mid = scratch_file(&setup.scratch, "before-cutout", "png");
                scratch.push(mid.clone());
                let mut a = s(&["-hide_banner", "-nostdin", "-y", "-i"]);
                a.push(source.display().to_string());
                a.extend(s(&["-frames:v", "1", "-update", "1", "-filter_complex"]));
                a.push(if pre.is_empty() { "null".into() } else { pre.join(",") });
                a.extend(s(&["-c:v", "png"]));
                a.push(mid.display().to_string());
                let out = run(ffmpeg, &a, LIMIT)?;
                if !out.status.success() || !mid.is_file() {
                    return Err(format!("ffmpeg stopped: {}", complaint(&out.stderr)));
                }
                source = mid;
                graph.clear();
            }
            let (mw, mh) = by.input_size(w, h);
            let small = small_copy(ffmpeg, &source, 0, Some((mw, mh)))?;
            let matte = crate::cutout::matte(by, &model, &small.small.rgb, small.small.width, small.small.height)?;
            let share = matte.subject_share();
            if !(0.01..=0.97).contains(&share) {
                return Ok(Outcome::Said(format!(
                    "{} couldn't find a clear subject in {} ({} of it looked like subject), so I didn't make a copy.",
                    by.plain(),
                    name_of(photo),
                    pct(share)
                )));
            }
            let mask = scratch_file(&setup.scratch, "matte", "pgm");
            scratch.push(mask.clone());
            write_mask(&matte, &mask)?;
            inputs = vec![source.display().to_string(), mask.display().to_string()];
            let tail = if post.is_empty() { String::new() } else { format!(",{}", post.join(",")) };
            complex = Some(match &bg {
                Op::BlurBackground => {
                    let sigma = (w.max(h) as f32 / 120.0).max(4.0);
                    said.push(format!("blurred the background ({} found the subject)", by.plain()));
                    format!(
                        "[1:v]scale={w}:{h}:flags=bicubic,format=gray,format=gbrp[m];[0:v]format=gbrp,split[a][b];\
                         [b]gblur=sigma={sigma:.1}[bl];[bl][a][m]maskedmerge{tail}[out]"
                    )
                }
                _ => {
                    said.push(format!("took the background out ({} found the subject)", by.plain()));
                    format!("[1:v]scale={w}:{h}:flags=bicubic,format=gray[m];[0:v]format=rgba[c];[c][m]alphamerge{tail}[out]")
                }
            });
        } else {
            complex = None;
            graph.extend(post.clone());
        }

        let copy = copy_path(photo, format);
        // JPEG quality: best first; a platform size limit steps it down.
        let steps: &[u32] = if max_bytes.is_some() && format == Format::Jpeg { &[2, 4, 6, 9, 13] } else { &[2] };
        for (i, q) in steps.iter().enumerate() {
            let mut a = s(&["-hide_banner", "-nostdin", "-n"]);
            for input in &inputs {
                a.push("-i".into());
                a.push(input.clone());
            }
            match &complex {
                Some(g) => {
                    a.push("-filter_complex".into());
                    a.push(g.clone());
                    a.extend(s(&["-map", "[out]"]));
                }
                None if !graph.is_empty() => {
                    a.push("-filter_complex".into());
                    a.push(graph.join(","));
                }
                None => {}
            }
            a.extend(s(&["-frames:v", "1", "-update", "1", "-map_metadata", "-1"]));
            a.extend(encode(format, *q));
            a.push(copy.display().to_string());
            let out = run(ffmpeg, &a, LIMIT)?;
            if !out.status.success() || !copy.is_file() {
                crate::heard!(std::fs::remove_file(&copy));
                return Err(format!("ffmpeg stopped: {}", complaint(&out.stderr)));
            }
            let size = std::fs::metadata(&copy).map(|m| m.len()).unwrap_or(0);
            match max_bytes {
                Some(max) if size > max && i + 1 < steps.len() => {
                    // Our own new file, made a moment ago: remade smaller.
                    crate::heard!(std::fs::remove_file(&copy));
                }
                Some(max) if size > max => {
                    said.push(format!("it's {:.1} MB, over the {} MB a phone can upload -- fine from a computer", size as f64 / 1e6, max / 1_000_000));
                    break;
                }
                _ => break,
            }
        }
        let mut head = if said.is_empty() { String::new() } else { format!(": {}", said.join(", ")) };
        if head.contains(AFTER) {
            // Said as measured on the copy, not as predicted.
            let after = look_at_photo(ffmpeg, &copy).map(|l| pct(stats(&l.small).mean)).unwrap_or_else(|_| "?".into());
            head = head.replace(AFTER, &after);
        }
        Ok(Outcome::Made {
            original: photo.to_path_buf(),
            said: format!("Made {} beside the original{head}. The original is untouched.", name_of(&copy)),
            copy,
        })
    })();
    for f in scratch {
        crate::heard!(std::fs::remove_file(f));
    }
    result
}

/// The photos in a folder (not its sub-folders), leaving out Atlas's own
/// edited copies, in name order.
fn photos_in(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).collect())
        .unwrap_or_default();
    v.retain(|p| p.is_file() && PHOTO_EXTS.contains(&ext_of(p).as_str()) && !name_of(p).contains(".edited"));
    v.sort();
    v
}

/// Do `wish` to every photo in `dir`, one at a time (two cores: in turn is
/// quicker than side by side). `stop` is asked between photos.
fn edit_folder(setup: &Setup, dir: &Path, wish: &Wish, stop: &dyn Fn() -> bool) -> (Vec<(PathBuf, PathBuf)>, String) {
    let all = photos_in(dir);
    let folder = name_of(dir);
    if all.is_empty() {
        return (Vec::new(), format!("There are no photos in {folder}."));
    }
    let mut made = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    let mut tilted: Vec<String> = Vec::new();
    let mut level = 0;
    let mut stopped = false;
    for p in &all {
        if stop() {
            stopped = true;
            break;
        }
        match edit_one(setup, p, wish, None, true) {
            Outcome::Made { original, copy, .. } => made.push((original, copy)),
            Outcome::Offered { photo, fix, .. } => tilted.push(format!("{} ({:.1}°)", name_of(&photo), fix.abs())),
            Outcome::Said(why) if wish.ops.contains(&Op::Straighten) && why.contains("looks level") => level += 1,
            Outcome::Said(why) => failed.push(why),
        }
    }
    let mut said = if wish.ops.contains(&Op::Straighten) {
        if tilted.is_empty() {
            format!("None of the {} photos in {folder} look off level enough to straighten ({level} measured level; the rest had too few straight lines to tell).", all.len())
        } else {
            format!(
                "{} of the {} photos in {folder} look off level: {}. Say \"straighten\" with one of them and I'll offer the fix for it.",
                tilted.len(),
                all.len(),
                tilted.join(", ")
            )
        }
    } else {
        format!(
            "Done {} of the {} photos in {folder}: each has an .edited copy beside it, and the originals are untouched.",
            made.len(),
            all.len()
        )
    };
    if let Some(first) = failed.first() {
        said.push_str(&format!(" {} couldn't be done -- the first: {first}", failed.len()));
    }
    if stopped {
        said.push_str(" I stopped when you asked, part way through.");
    }
    (made, said)
}

/// Take the last edit back: delete the copies it made. Originals are never
/// touched; the sentence says where they are.
pub fn undo_photo_edit(state: &Path) -> String {
    let mut mem = Memory::load(state);
    if mem.made.is_empty() {
        return "There's no photo edit of mine to take back.".into();
    }
    let made = std::mem::take(&mut mem.made);
    let mut gone = 0;
    let mut missing = 0;
    for (_, copy) in &made {
        let p = Path::new(copy);
        if p.is_file() && std::fs::remove_file(p).is_ok() {
            gone += 1;
        } else {
            missing += 1;
        }
    }
    let original = made[0].0.clone();
    mem.last = Some((original.clone(), original.clone()));
    mem.save(state);
    let already = if missing > 0 { format!(" ({missing} had already gone.)") } else { String::new() };
    if made.len() == 1 {
        format!("Deleted {}. What it was made from is untouched, where it was: {original}.{already}", name_of(Path::new(&made[0].1)))
    } else {
        let dir = Path::new(&original).parent().map(|d| d.display().to_string()).unwrap_or_default();
        format!("Deleted the {gone} edited copies. The originals are where they were, in {dir}.{already}")
    }
}

/// What to do about an ask: answered now, or work for the crew with the
/// sentence to say while it runs.
pub enum Plan {
    Now(String),
    Later { start: String, work: Box<dyn FnOnce(&dyn Fn() -> bool) -> String + Send> },
}

/// A folder named in what was said: quoted, or a word that is one.
fn folder_in(said: &str) -> Option<PathBuf> {
    for q in ['"', '\''] {
        let mut parts = said.split(q);
        parts.next();
        if let Some(p) = parts.next() {
            let p = PathBuf::from(p.trim());
            if p.is_dir() {
                return Some(p);
            }
        }
    }
    said.split_whitespace().map(|w| PathBuf::from(w.trim_matches(|c: char| c == ',' || c == '?' || c == '!'))).find(|p| p.is_dir() && p.is_absolute())
}

/// A photo's path in the words: `files::path_in`, kept only when it names a
/// photo or a file on disk (a folder is `folder_in`'s). `path_in` takes any quoted stretch, and two
/// apostrophes ("brighten Maya's photo, it's dark") quote one.
fn photo_path_in(said: &str) -> Option<String> {
    crate::files::path_in(said, PHOTO_EXTS).filter(|p| {
        let p = Path::new(p.trim());
        PHOTO_EXTS.contains(&ext_of(p).as_str()) || p.is_file()
    })
}

/// A copied path, if it is one photo on this machine: what Explorer's "Copy
/// as path" puts on the clipboard (quoted), or a path typed and copied.
fn copied_photo(copied: &str) -> Option<String> {
    let t = copied.trim().trim_matches('"').trim();
    if t.is_empty() || t.lines().count() != 1 {
        return None;
    }
    let p = Path::new(t);
    (p.is_file() && PHOTO_EXTS.contains(&ext_of(p).as_str())).then(|| t.to_string())
}

/// Which photo "this photo" means, in order: a path in the words; what was
/// copied, when the words say so ("the photo I copied"); the last photo
/// handed to Atlas (`handed`: the tray); and last, a copied photo path when
/// the words only say "this" or "that". `copied` is the clipboard's text,
/// read only because the words pointed at it (`clipboard::refers_to_clipboard`).
/// `None` from all of them is a question, asked by `ask`.
pub fn which_photo(said: &str, copied: Option<&str>, handed: Option<String>) -> Option<String> {
    if let Some(p) = photo_path_in(said) {
        return Some(p);
    }
    let copied = copied.and_then(copied_photo);
    // The daemon's `file_meant` tries the words first too, so what it hands
    // over can be the same false "quoted" path: kept only if it's a photo.
    let handed = handed.filter(|h| {
        let p = Path::new(h.trim());
        PHOTO_EXTS.contains(&ext_of(p).as_str()) || p.is_file()
    });
    let l = said.to_lowercase();
    if has(&l, &["copied", "clipboard", "pasted"]) {
        copied.or(handed)
    } else {
        handed.or(copied)
    }
}

/// Answer an ask about photos. `handed` is the photo the words point at, or
/// the last one handed to Atlas; `said` is the whole ask.
pub fn ask(said: &str, handed: Option<String>, setup: Setup) -> Plan {
    let named_file = photo_path_in(said);
    let named_dir = folder_in(said);
    let mut words = said.to_string();
    for p in named_file.iter().cloned().chain(named_dir.iter().map(|d| d.display().to_string())) {
        words = words.replace(&p, " ");
    }
    let mut wish = read_wish(&words);
    let mut mem = Memory::load(&setup.state);
    // "Do the same to the whole folder": the last edit's words, again, on
    // whatever this ask names -- a folder, or another photo.
    let l = format!(" {} ", words.to_lowercase());
    if wish.ops.is_empty() && wish.format.is_none() && !wish.undo && !wish.get_models && has(&l, &[" the same", " same again", " that again", " do that to", " same thing"]) {
        let Some(before) = mem.last_ask.clone() else {
            return Plan::Now("I haven't edited a photo yet, so there's nothing to do the same as -- tell me what to change.".into());
        };
        let folder = wish.folder;
        wish = read_wish(&before);
        // Never a straightening: that's measured photo by photo and isn't
        // kept here (see below).
        wish.folder |= folder;
        words = before;
    }
    if wish.undo {
        return Plan::Now(undo_photo_edit(&setup.state));
    }
    if wish.get_models {
        if crate::cutout::installed(&setup.models).is_some() && crate::getpieces::photos().iter().all(|p| crate::getpieces::have(p, &setup.install)) {
            return Plan::Now("The photo models are already here.".into());
        }
        let install = setup.install.clone();
        return Plan::Later {
            start: "Fetching the photo cut-out models -- I'll say when they're in.".into(),
            work: Box::new(move |_| {
                let tools = crate::getpieces::Tools::default();
                for p in crate::getpieces::photos() {
                    if let Err(why) = crate::getpieces::fetch(&p, &install, &tools, &|_, _| {}) {
                        return format!("I couldn't fetch {}: {why}", p.name);
                    }
                }
                "The photo models are in: I can blur or remove a background now.".into()
            }),
        };
    }
    if wish.ops.is_empty() && wish.format.is_none() {
        return Plan::Now(
            "What should I do to it? I can make it brighter or darker, fix the colours, straighten it, turn or flip it, \
             crop it for Instagram, a YouTube thumbnail or a story, resize it (\"1920 wide\", \"half size\"), denoise or sharpen it, \
             blur or remove the background, or save it as JPEG, PNG or WebP."
                .into(),
        );
    }
    if !ffmpeg_here(&setup.ffmpeg) {
        return Plan::Now("Editing photos needs ffmpeg, which Atlas's setup fetches, and it isn't on this machine yet.".into());
    }
    // Kept for "do the same": an edit, not a measurement or a yes to one.
    if !wish.ops.iter().any(|o| matches!(o, Op::Straighten | Op::StraightenAccepted)) {
        mem.last_ask = Some(words.split_whitespace().collect::<Vec<_>>().join(" "));
        mem.save(&setup.state);
    }

    if wish.folder {
        let dir = named_dir
            .or_else(|| named_file.as_ref().and_then(|f| Path::new(f).parent().map(Path::to_path_buf)))
            .or_else(|| handed.as_ref().and_then(|f| Path::new(f).parent().map(Path::to_path_buf)))
            .or_else(|| mem.last.as_ref().and_then(|(o, _)| Path::new(o).parent().map(Path::to_path_buf)));
        let Some(dir) = dir.filter(|d| d.is_dir()) else {
            return Plan::Now("Which folder? Give me its path.".into());
        };
        let n = photos_in(&dir).len();
        if n == 0 {
            return Plan::Now(format!("There are no photos in {}.", dir.display()));
        }
        let start = format!("Working through the {n} photos in {} -- each gets a new copy beside it; the originals aren't touched.", name_of(&dir));
        return Plan::Later {
            start,
            work: Box::new(move |stop| {
                let (made, said) = edit_folder(&setup, &dir, &wish, stop);
                if !made.is_empty() {
                    let mut mem = Memory::load(&setup.state);
                    mem.made = made.iter().map(|(o, c)| (o.display().to_string(), c.display().to_string())).collect();
                    mem.save(&setup.state);
                }
                said
            }),
        };
    }

    // One photo: the one named; else, if the one handed over is the one the
    // last edit started from, carry on from that edit's copy ("now crop it");
    // else the one handed over; else the last one worked on.
    let photo = named_file.or_else(|| match (&handed, &mem.last) {
        (Some(h), Some((orig, copy))) if h == orig && Path::new(copy).is_file() => Some(copy.clone()),
        (Some(h), _) => Some(h.clone()),
        (None, Some((_, copy))) if Path::new(copy).is_file() => Some(copy.clone()),
        (None, Some((orig, _))) => Some(orig.clone()),
        (None, None) => None,
    });
    let offer = mem.offer.clone();
    let photo = match (&photo, &offer) {
        // "Yes, straighten it" is about the photo it was offered for.
        (_, Some((p, _))) if wish.ops.contains(&Op::StraightenAccepted) => PathBuf::from(p),
        (Some(p), _) => PathBuf::from(p),
        (None, _) => return Plan::Now("Which photo? Give me its path, or hand it to me first.".into()),
    };
    if wish.ops.contains(&Op::StraightenAccepted) && offer.is_none() {
        return Plan::Now("I haven't measured a photo to straighten yet -- say \"straighten\" with the photo and I'll see how far off it is.".into());
    }
    let start = if wish.ops.contains(&Op::Straighten) {
        format!("Measuring how level {} is.", name_of(&photo))
    } else if wish.ops.iter().any(|o| matches!(o, Op::BlurBackground | Op::RemoveBackground)) {
        // Measured 29 Sep 2026 on two cores, optimised build: MODNet about
        // 3 s for a 768x512 matte, u2netp about 2 s, plus a quarter second
        // to load either.
        format!("Working on a copy of {} -- finding the subject takes a few seconds; the original isn't touched.", name_of(&photo))
    } else {
        format!("Working on a copy of {} -- the original isn't touched.", name_of(&photo))
    };
    Plan::Later {
        start,
        work: Box::new(move |_| {
            let fix = offer.as_ref().filter(|(p, _)| Path::new(p) == photo).map(|(_, f)| *f);
            let out = edit_one(&setup, &photo, &wish, fix, false);
            let mut mem = Memory::load(&setup.state);
            let said = match out {
                Outcome::Made { original, copy, said } => {
                    // The chain goes back to the first original, so "undo"
                    // and "now crop it" both know where it started.
                    let root = match &mem.last {
                        Some((o, c)) if Path::new(c) == original => o.clone(),
                        _ => original.display().to_string(),
                    };
                    mem.made = vec![(original.display().to_string(), copy.display().to_string())];
                    mem.last = Some((root, copy.display().to_string()));
                    if wish.ops.contains(&Op::StraightenAccepted) {
                        mem.offer = None;
                    }
                    said
                }
                Outcome::Offered { photo, fix, said } => {
                    mem.offer = Some((photo.display().to_string(), fix));
                    said
                }
                Outcome::Said(s) => s,
            };
            mem.save(&setup.state);
            said
        }),
    }
}

/// Is ffmpeg where it's said to be (a path) or on the PATH (a name)?
fn ffmpeg_here(ffmpeg: &str) -> bool {
    crate::tools::which(ffmpeg).is_some()
}
