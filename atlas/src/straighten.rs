//! Is this photo level? A measured guess, offered and never applied unasked.
//!
//! No small, freely licensed horizon model exists (R5 research, 28 Sep 2026),
//! so this is the classical way: find the edges, see which way they run, and
//! look for a dominant direction a few degrees off horizontal or vertical.
//! Buildings, horizons, desks, door frames and shelves give one; a face in
//! close-up or a plate of food doesn't, and then this says it can't tell
//! rather than guessing.
//!
//! 1. The picture, small (about 512 px) and grey, softened a little.
//! 2. Gradients (Scharr). Every strong edge has a direction and a strength.
//! 3. Horizontal and vertical edges are folded together (a wall and a floor
//!    tilt the same way when the camera does), and only directions within
//!    `MAX_TILT` of level count.
//! 4. Their average direction (see `measure` for how, and why not the
//!    plain average) is the tilt; the share of all the edge strength that
//!    agrees with it is the confidence.
//!
//! People disagree with automatic straightening often enough (a deliberate
//! Dutch angle, a sloping street) that Atlas only ever *offers* it:
//! "it looks 2.3° off -- straighten it?".

/// Tilts further than this are left alone: a photo 20° off was taken that
/// way on purpose, and the edges of a sloping roof would pass for it.
pub const MAX_TILT: f32 = 10.0;

/// How far from the answer an edge may lean and still count towards it.
const WINDOW: f32 = 4.0;

/// Below this the photo is called level.
pub const LEVEL_ENOUGH: f32 = 0.3;

/// A measured tilt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tilt {
    /// Degrees to turn the photo **clockwise** to level it (negative:
    /// anticlockwise). The sign ffmpeg's `rotate` takes.
    pub fix: f32,
    /// How much of the edge evidence agrees, 0..1.
    pub confidence: f32,
    /// How many edge pixels voted.
    pub edges: usize,
}

impl Tilt {
    /// Enough agreement to offer a fix.
    pub fn sure(&self) -> bool {
        self.edges >= 200 && self.confidence >= 0.2
    }

    /// Already level, as far as can be told.
    pub fn level(&self) -> bool {
        self.fix.abs() < LEVEL_ENOUGH
    }
}

/// Grey from interleaved RGB (Rec. 601 weights).
pub fn grey(rgb: &[u8]) -> Vec<f32> {
    rgb.chunks_exact(3).map(|p| 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32).collect()
}

/// Measure the tilt of a grey picture `w` x `h`. `None` when there are
/// hardly any edges to go on.
///
/// Averaging each pixel's angle was tried first and was wrong by a third of
/// a degree at 6°, and by more near level: along a line a few degrees off,
/// the edge's pixel phase wobbles, and single-pixel angles scatter towards
/// the axes. What's averaged instead is the direction itself, with the angle
/// taken four times over (so horizontal and vertical edges, which lean
/// together, add up rather than cancel) and each edge weighted by its
/// strength squared -- the structure tensor, folded for right angles. Then
/// again over only the edges within `WINDOW` of that answer, three times, so
/// a sloping roof or a road's perspective further off pulls it less. On
/// lines drawn at known angles (tests/photo_editing.rs) it's within 0.15°.
/// Narrower windows were tried and were worse: single-pixel angles scatter
/// lopsidedly, and cutting the scatter at 2° cut it unevenly (-2.3° read as
/// -1.6°). Two strong sets of lines 7° apart still pull each other; the
/// confidence drops when they do, and it's only ever offered.
pub fn measure(g: &[f32], w: usize, h: usize) -> Option<Tilt> {
    if w < 16 || h < 16 || g.len() != w * h {
        return None;
    }
    // Softened first: a hard-edged line a few degrees off is a staircase of
    // level steps.
    let soft = soften(&soften(g, w, h), w, h);
    let at = |x: usize, y: usize| soft[y * w + x];
    // Scharr's gradient (3-10-3): closer to the same answer in every
    // direction than Sobel's 1-2-1.
    let mut edges: Vec<(f32, f32)> = Vec::with_capacity(w * h / 4); // (lean in degrees, strength^2)
    let mut strongest = 0f32;
    let mut all = Vec::with_capacity(w * h);
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let gx = 3.0 * (at(x + 1, y - 1) + at(x + 1, y + 1)) + 10.0 * at(x + 1, y)
                - 3.0 * (at(x - 1, y - 1) + at(x - 1, y + 1))
                - 10.0 * at(x - 1, y);
            // y grows downwards in the picture; flipped so angles are the
            // usual anticlockwise-positive ones.
            let gy = 3.0 * (at(x - 1, y - 1) + at(x + 1, y - 1)) + 10.0 * at(x, y - 1)
                - 3.0 * (at(x - 1, y + 1) + at(x + 1, y + 1))
                - 10.0 * at(x, y + 1);
            let m = (gx * gx + gy * gy).sqrt();
            strongest = strongest.max(m);
            // The edge runs at right angles to its gradient; folded to within
            // 45° of the nearest of horizontal and vertical.
            let lean = (gy.atan2(gx).to_degrees() + 90.0 + 45.0).rem_euclid(90.0) - 45.0;
            all.push((lean, m));
        }
    }
    // An edge must stand out: a tenth of the strongest, and never less than
    // a plain floor, so sensor noise on a flat wall doesn't vote.
    let floor = (strongest * 0.1).max(200.0);
    let mut total = 0f64;
    for &(lean, m) in &all {
        if m < floor {
            continue;
        }
        total += (m * m) as f64;
        if lean.abs() <= MAX_TILT {
            edges.push((lean, m * m));
        }
    }
    if edges.len() < 20 || total <= 0.0 {
        return None;
    }
    let average = |near: Option<f32>| -> Option<(f32, f64)> {
        let (mut c, mut s, mut wsum) = (0f64, 0f64, 0f64);
        for &(lean, wt) in &edges {
            if near.is_some_and(|n| (lean - n).abs() > WINDOW) {
                continue;
            }
            let t = (4.0 * lean as f64).to_radians();
            c += wt as f64 * t.cos();
            s += wt as f64 * t.sin();
            wsum += wt as f64;
        }
        (wsum > 0.0).then(|| ((s.atan2(c).to_degrees() / 4.0) as f32, wsum))
    };
    let mut lean = average(None)?.0;
    let mut support = 0f64;
    for _ in 0..3 {
        let (l, sup) = average(Some(lean))?;
        lean = l;
        support = sup;
    }
    // Confidence: the strength within `WINDOW` of the answer, of all the
    // edges in every direction. Noise spreads over all 90°, so gets about 9%.
    let n = edges.iter().filter(|(l, _)| (l - lean).abs() <= WINDOW).count();
    // The edges lean `lean` degrees anticlockwise; turning the photo that
    // far clockwise levels them.
    Some(Tilt { fix: lean, confidence: (support / total) as f32, edges: n })
}

/// A 1-2-1 blur across and down.
fn soften(g: &[f32], w: usize, h: usize) -> Vec<f32> {
    let mut across = vec![0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let l = g[y * w + x.saturating_sub(1)];
            let r = g[y * w + (x + 1).min(w - 1)];
            across[y * w + x] = (l + 2.0 * g[y * w + x] + r) / 4.0;
        }
    }
    let mut out = vec![0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let u = across[y.saturating_sub(1) * w + x];
            let d = across[(y + 1).min(h - 1) * w + x];
            out[y * w + x] = (u + 2.0 * across[y * w + x] + d) / 4.0;
        }
    }
    out
}

/// The offer, in words. `name` is the photo's file name.
pub fn offer(t: &Tilt, name: &str) -> String {
    if !t.sure() {
        return format!(
            "I can't tell which way is level in {name} -- there aren't enough straight lines in it to go on. \
             If you know it's off, say \"rotate it 2 degrees clockwise\" (or anticlockwise) and I'll make a copy turned that much."
        );
    }
    if t.level() {
        return format!("{name} already looks level (within {:.1}°), so I've left it alone.", t.fix.abs());
    }
    format!(
        "{name} looks {:.1}° off -- it leans {}. Straighten it? Say \"yes, straighten it\" and I'll make a straightened copy beside it; the original stays as it is.",
        t.fix.abs(),
        if t.fix > 0.0 { "anticlockwise, so it needs turning clockwise" } else { "clockwise, so it needs turning anticlockwise" }
    )
}

/// How much of a `w` x `h` picture survives turning it `degrees` and
/// cropping to the largest rectangle of the same shape inside: the scale of
/// the crop, 0..1. Turning leaves empty corners; this is how much to trim so
/// none show.
pub fn crop_after_turning(w: f64, h: f64, degrees: f64) -> f64 {
    let (s, c) = degrees.to_radians().abs().sin_cos();
    let a = w / (w * c + h * s);
    let b = h / (w * s + h * c);
    a.min(b).clamp(0.0, 1.0)
}
