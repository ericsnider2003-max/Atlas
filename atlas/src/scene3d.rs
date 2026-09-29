//! 3-D scenes that move, drawn in house — and handed to Blender when it's there.
//!
//! A scene is JSON: shapes with colours, a sun, a sky, a camera — and, when it
//! has a `duration`, a timeline. Anything can move: an object's position,
//! turn, size and colour, the camera's position, target and lens, each by
//! keyframes with an easing, or by `spin` (degrees a second) and a camera
//! `orbit`. Still scenes come out as a picture and a turntable; moving ones as
//! a GIF, an MP4 (with ffmpeg) and the frames.
//!
//! **How it's drawn.** A ray tracer, written here, with the parts that make a
//! picture read as solid rather than as flat shapes:
//!
//! - **Shapes** — sphere, box, cylinder, cone, capsule, torus and a ground —
//!   each turned (`rotate`, degrees about x, y, z) and sized (`scale`) by its
//!   own transform, so a box can tumble. The torus is found by sphere tracing
//!   its distance function (Hart 1996); the rest are solved exactly.
//! - **Light** — a sun with a real size, so shadows go soft at the edges;
//!   light from the whole sky, darkened where it can't reach (ambient
//!   occlusion); a highlight, and reflections weighted by angle (Schlick's
//!   Fresnel) on shiny things; haze toward the horizon.
//! - **Film** — several jittered rays a pixel on a fixed pattern (so still
//!   parts of a moving scene don't shimmer), each lighting sample spread across
//!   them; the camera's shutter open for half a frame, so fast things blur the
//!   way they do on film; then the ACES filmic curve (Narkowicz's fit) and the
//!   sRGB gamma, so bright light rolls off instead of clipping to white.
//! - Rows are drawn on every core.
//!
//! **Keyframes** follow glTF 2.0's animation model: a channel (`prop`) has
//! times and values, and between two keys the value is interpolated — here
//! with an easing from Penner's set (linear, step, ease-in/out, bounce, back,
//! elastic). Values are positions, angles, a size or a colour.
//!
//! **Blender** gets the same scene, motion baked frame by frame from this
//! module's own timeline — so what Blender renders moves exactly as the
//! in-house render does — as a Python script it runs headless. What comes back
//! is checked like every render: it exists, it is the size and the frame
//! count asked.
//!
//! **What's checked** before and after drawing: the scene reads, the timeline
//! is sane, something is in view, every object stays in frame (or it's said
//! when one leaves), and a scene that claims to move does move. Whether it
//! looks good stays with you.
//!
//! Sources: Shirley, *Ray Tracing in One Weekend* series (CC0); Blinn 1977;
//! Schlick 1994; Hart 1996 (sphere tracing); Khronos glTF 2.0 §3.11
//! (animations); Penner's easing equations; Narkowicz 2016 (ACES fit);
//! Blender's Python API (`keyframe_insert`, `render.render(animation=True)`);
//! round 8: Snell's law with Schlick's Fresnel for glass; next-event
//! estimation by uniform cone sampling for lamps (PBRT 4ed §12, §13.2);
//! Roberts' R2 sequence; Dammertz, Sewtz, Hanika & Lensch 2010 (edge-avoiding
//! à-trous) and Schied et al. 2017 (SVGF variance steering) for the denoiser;
//! model files through `meshio`.

use crate::motion::{Finding, Severity};
use crate::pngcodec::Rgba;
use serde::{Deserialize, Serialize};

type V = [f64; 3];

// ---------------------------------------------------------------- the scene

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Camera {
    /// Where the camera stands.
    pub from: V,
    /// What it looks at.
    #[serde(default)]
    pub at: V,
    /// Vertical field of view, degrees.
    #[serde(default = "default_fov")]
    pub fov: f64,
    /// Degrees to circle around `at` over the whole duration (0 = stay put).
    #[serde(default)]
    pub orbit: f64,
    /// Keyframes for `from`, `at` or `fov`.
    #[serde(default)]
    pub animate: Vec<Track>,
}

fn default_fov() -> f64 {
    40.0
}

/// The geometry. Where it is, how it's turned and sized, and what colour, are
/// on the [`Object`] that holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "lowercase")]
pub enum Shape {
    Sphere { radius: f64 },
    /// `size` is width, height and depth; `at` is its centre.
    Box { size: V },
    /// Stands on `at` (its base centre).
    Cylinder { radius: f64, height: f64 },
    /// Stands on `at`; its point is `height` above.
    Cone { radius: f64, height: f64 },
    /// A cylinder with round ends, standing on `at`; `height` is overall.
    Capsule { radius: f64, height: f64 },
    /// A ring lying flat around `at`: `radius` to the middle of the tube,
    /// `thickness` the tube's radius.
    Torus { radius: f64, thickness: f64 },
    /// A model from a file: OBJ (with its MTL colours), STL, or glTF/GLB.
    /// It stands on `at`, centred over it. `fit` sizes it so its longest side
    /// is that many metres; without it the file's own units are taken as
    /// metres. A relative `file` is found beside the scene file (or in
    /// Atlas's `models` folder for a scene Atlas drafted).
    Mesh {
        file: String,
        #[serde(default)]
        fit: Option<f64>,
    },
    /// The ground: an endless floor at height `y`.
    Ground {
        #[serde(default)]
        y: f64,
        /// "checker" (the default) or "plain".
        #[serde(default)]
        pattern: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Object {
    #[serde(flatten)]
    pub shape: Shape,
    #[serde(default)]
    pub at: V,
    /// Degrees about x, then y, then z.
    #[serde(default)]
    pub rotate: V,
    #[serde(default = "one")]
    pub scale: f64,
    #[serde(default = "grey")]
    pub colour: String,
    /// 0 is matte; 1 is a mirror-like finish.
    #[serde(default)]
    pub shine: f64,
    /// A name to refer to it by, if you like.
    #[serde(default)]
    pub name: Option<String>,
    /// Degrees a second about x, y, z — a steady turn with no keyframes.
    #[serde(default)]
    pub spin: V,
    /// Keyframes for `at`, `rotate`, `scale` or `colour`.
    #[serde(default)]
    pub animate: Vec<Track>,
    /// What it's made of: "solid" (the default), "glass" or "glow".
    #[serde(default)]
    pub material: Material,
    /// Glass: how strongly it bends light (1.5 window glass, 1.33 water,
    /// 2.4 diamond).
    #[serde(default)]
    pub ior: Option<f64>,
    /// Glow: how bright (3 when not given).
    #[serde(default)]
    pub glow: Option<f64>,
    /// A pattern on its surface, fixed to the object so it turns with it.
    #[serde(default)]
    pub pattern: Option<Pattern>,
}

/// What an object is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Material {
    /// Lit by the sun and sky; `shine` makes it glossy.
    #[default]
    Solid,
    /// Clear, bending and reflecting light (Fresnel); tinted by its colour;
    /// its shadow lets light through.
    Glass,
    /// Gives off light in its colour: a lamp, a screen, embers.
    Glow,
}

/// A pattern: `"checker"`, or `{"kind":"stripes","colour":"#223344","size":0.2}`.
/// Kinds: checker, stripes (bands up the object), grid (lines), dots, noise.
/// `colour` is the second colour (the object's own is the first); `size` is
/// one square, band or dot spacing in metres, before the object's `scale`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Pattern {
    Kind(String),
    Full {
        kind: String,
        #[serde(default)]
        colour: Option<String>,
        #[serde(default)]
        size: Option<f64>,
    },
}

fn one() -> f64 {
    1.0
}
fn grey() -> String {
    "#b8b8b8".into()
}

/// How the sun lights things.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Sun {
    /// Just a direction the light comes from.
    From(V),
    Full {
        from: V,
        /// Its apparent size in degrees: 0 gives hard shadows, the real sun
        /// is about 0.5, a hazy day or a big lamp 5 to 10.
        #[serde(default = "default_softness")]
        softness: f64,
        #[serde(default = "default_sun_colour")]
        colour: String,
        #[serde(default = "one")]
        strength: f64,
    },
}

fn default_softness() -> f64 {
    4.0
}
fn default_sun_colour() -> String {
    "#fff4e0".into()
}

/// How many rays a pixel: more is smoother and slower.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    /// 4 rays a pixel: for trying things.
    Draft,
    /// 9: the default.
    #[default]
    Good,
    /// 16: for keeping.
    Best,
    /// 64: slow; what the others are measured against.
    Reference,
}

impl Quality {
    fn side(self) -> usize {
        match self {
            Quality::Draft => 2,
            Quality::Good => 3,
            Quality::Best => 4,
            Quality::Reference => 8,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    #[serde(default = "default_w")]
    pub width: u32,
    #[serde(default = "default_h")]
    pub height: u32,
    pub camera: Camera,
    #[serde(default = "default_sun")]
    pub sun: Sun,
    /// The sky at the horizon.
    #[serde(default = "default_sky")]
    pub sky: String,
    /// The sky overhead; a deeper version of `sky` when not given.
    #[serde(default)]
    pub sky_top: Option<String>,
    pub objects: Vec<Object>,
    /// Seconds. 0 is a still picture.
    #[serde(default)]
    pub duration: f64,
    #[serde(default = "default_fps")]
    pub fps: u32,
    /// Blur what moves fast, as a film camera does (on unless turned off).
    #[serde(default = "yes")]
    pub motion_blur: bool,
    #[serde(default)]
    pub quality: Quality,
    /// Smooth away the grain after drawing, keeping edges (Dammertz et al.'s
    /// edge-avoiding à-trous filter). On for "draft" unless `false`; `true`
    /// turns it on at any quality.
    #[serde(default)]
    pub denoise: Option<bool>,
    /// More rays where a pixel's first rays disagree (soft shadows, lamp
    /// light), none extra where they agree. Off unless `true`: it was
    /// measured to help a little on noisy light and to lose on fine
    /// patterns (see `ADAPT_REL`).
    #[serde(default)]
    pub adaptive: Option<bool>,
    /// Where the scene's relative model files are looked for: the scene
    /// file's folder. Not part of the JSON; [`load_scene`] sets it.
    #[serde(skip)]
    pub base: Option<std::path::PathBuf>,
}

fn default_w() -> u32 {
    480
}
fn default_h() -> u32 {
    360
}
fn default_sun() -> Sun {
    Sun::From([-0.5, 1.0, 0.6])
}
fn default_sky() -> String {
    "#dfe8f2".into()
}
fn default_fps() -> u32 {
    24
}
fn yes() -> bool {
    true
}

// ----------------------------------------------------------- the timeline

/// One animated property: keyframes `[time, value]` and how to get between
/// them. A value is a number, `[x, y, z]`, or a colour string.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    /// `at`, `rotate`, `scale`, `colour` on an object; `from`, `at`, `fov`
    /// on the camera.
    pub prop: String,
    pub keys: Vec<(f64, serde_json::Value)>,
    #[serde(default)]
    pub ease: Ease,
    /// Play the keys over and over: the timeline wraps at the last key.
    #[serde(default, rename = "loop")]
    pub repeat: bool,
}

/// How a value travels between two keys (Penner's easing families).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Ease {
    Linear,
    /// Holds each key until the next.
    Step,
    EaseIn,
    EaseOut,
    /// Slow at both ends — the default, because it's how things move.
    #[default]
    EaseInOut,
    /// Lands and bounces to rest.
    Bounce,
    /// Overshoots a little and settles.
    Back,
    /// Springs past and wobbles in.
    Elastic,
}

/// Where along a segment, 0..1 in, the value is, 0..1 (can overshoot for
/// back and elastic).
fn eased(e: Ease, x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    match e {
        Ease::Linear => x,
        Ease::Step => {
            if x < 1.0 {
                0.0
            } else {
                1.0
            }
        }
        Ease::EaseIn => x * x * x,
        Ease::EaseOut => 1.0 - (1.0 - x).powi(3),
        Ease::EaseInOut => {
            if x < 0.5 {
                4.0 * x * x * x
            } else {
                1.0 - (-2.0 * x + 2.0).powi(3) / 2.0
            }
        }
        Ease::Bounce => bounce_out(x),
        Ease::Back => {
            let c1 = 1.70158;
            let c3 = c1 + 1.0;
            1.0 + c3 * (x - 1.0).powi(3) + c1 * (x - 1.0).powi(2)
        }
        Ease::Elastic => {
            if x == 0.0 || x == 1.0 {
                x
            } else {
                let c4 = std::f64::consts::TAU / 3.0;
                2f64.powf(-10.0 * x) * ((x * 10.0 - 0.75) * c4).sin() + 1.0
            }
        }
    }
}

fn bounce_out(x: f64) -> f64 {
    let (n1, d1) = (7.5625, 2.75);
    if x < 1.0 / d1 {
        n1 * x * x
    } else if x < 2.0 / d1 {
        let x = x - 1.5 / d1;
        n1 * x * x + 0.75
    } else if x < 2.5 / d1 {
        let x = x - 2.25 / d1;
        n1 * x * x + 0.9375
    } else {
        let x = x - 2.625 / d1;
        n1 * x * x + 0.984375
    }
}

/// A key's value as three numbers (a single number fills all three; a colour
/// becomes its linear RGB).
fn value3(v: &serde_json::Value) -> Option<V> {
    match v {
        serde_json::Value::Number(n) => n.as_f64().map(|x| [x, x, x]),
        serde_json::Value::Array(a) if a.len() == 3 => {
            let f: Vec<f64> = a.iter().filter_map(|x| x.as_f64()).collect();
            (f.len() == 3).then(|| [f[0], f[1], f[2]])
        }
        serde_json::Value::String(s) => Some(linear(s)),
        _ => None,
    }
}

/// A track's value at time `t`.
fn sample(track: &Track, t: f64) -> Option<V> {
    let keys: Vec<(f64, V)> = track.keys.iter().filter_map(|(k, v)| value3(v).map(|v| (*k, v))).collect();
    let first = keys.first()?;
    let last = keys.last()?;
    let mut t = t;
    if track.repeat && last.0 > first.0 {
        let span = last.0 - first.0;
        t = first.0 + (t - first.0).rem_euclid(span);
    }
    if t <= first.0 {
        return Some(first.1);
    }
    if t >= last.0 {
        return Some(last.1);
    }
    let i = keys.windows(2).position(|w| t >= w[0].0 && t < w[1].0)?;
    let (a, b) = (keys[i], keys[i + 1]);
    let k = eased(track.ease, (t - a.0) / (b.0 - a.0).max(1e-9));
    Some(add(a.1, mul(sub(b.1, a.1), k)))
}

/// The tracks that decide each property at time `t`. Several tracks may key
/// one property one after another (a bounce down, then a rise back): the one
/// whose keys span `t` decides, and between or beyond them the nearest one
/// in time holds its end.
fn deciding(tracks: &[Track], t: f64) -> Vec<&Track> {
    let mut out: Vec<&Track> = Vec::new();
    for tr in tracks {
        let span = |tr: &Track| {
            let (a, b) = (tr.keys.first().map(|k| k.0).unwrap_or(0.0), tr.keys.last().map(|k| k.0).unwrap_or(0.0));
            if tr.repeat || (t >= a && t <= b) {
                0.0
            } else if t < a {
                a - t
            } else {
                t - b
            }
        };
        match out.iter().position(|o| o.prop == tr.prop) {
            Some(i) if span(tr) < span(out[i]) => out[i] = tr,
            Some(_) => {}
            None => out.push(tr),
        }
    }
    out
}

/// An object as it stands at time `t`: every track and spin applied.
fn object_at(o: &Object, t: f64) -> Object {
    let mut now = o.clone();
    let mut colour: Option<V> = None;
    for tr in deciding(&o.animate, t) {
        let Some(v) = sample(tr, t) else { continue };
        match tr.prop.as_str() {
            "at" | "position" => now.at = v,
            "rotate" | "rotation" => now.rotate = v,
            "scale" | "size" => now.scale = v[0],
            "colour" | "color" => colour = Some(v),
            _ => {}
        }
    }
    now.rotate = add(now.rotate, mul(o.spin, t));
    if let Some(c) = colour {
        now.colour = format!("lin:{},{},{}", c[0], c[1], c[2]);
    }
    now
}

/// The camera at time `t`: keyframes, then the orbit.
fn camera_at(c: &Camera, duration: f64, t: f64) -> Camera {
    let mut now = c.clone();
    for tr in deciding(&c.animate, t) {
        let Some(v) = sample(tr, t) else { continue };
        match tr.prop.as_str() {
            "from" | "position" => now.from = v,
            "at" | "target" => now.at = v,
            "fov" => now.fov = v[0],
            _ => {}
        }
    }
    if c.orbit != 0.0 && duration > 0.0 {
        let a = (c.orbit * t / duration).to_radians();
        let off = sub(now.from, now.at);
        let (s, co) = a.sin_cos();
        now.from = add(now.at, [off[0] * co - off[2] * s, off[1], off[0] * s + off[2] * co]);
    }
    now
}

/// The whole scene frozen at time `t` — what a frame is drawn from.
fn scene_at(scene: &Scene, t: f64) -> Scene {
    let mut s = scene.clone();
    s.camera = camera_at(&scene.camera, scene.duration, t);
    s.objects = scene.objects.iter().map(|o| object_at(o, t)).collect();
    s
}

/// The frame times: one per frame, from 0, not reaching the end (a loop's end
/// is its start). A still scene has one frame at 0.
fn frame_times(scene: &Scene) -> Vec<f64> {
    if scene.duration <= 0.0 {
        return vec![0.0];
    }
    let fps = scene.fps.clamp(1, 60) as f64;
    let n = ((scene.duration * fps).round() as usize).clamp(1, 1800);
    (0..n).map(|i| i as f64 / fps).collect()
}

// ------------------------------------------------------------------ vectors

fn add(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mul(a: V, k: f64) -> V {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn had(a: V, b: V) -> V {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2]]
}
fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V, b: V) -> V {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn len(a: V) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: V) -> V {
    let l = len(a);
    if l == 0.0 {
        a
    } else {
        mul(a, 1.0 / l)
    }
}

/// A rotation as three rows. Rx, then Ry, then Rz (Blender's "XYZ" order).
#[derive(Clone, Copy, Debug, PartialEq)]
struct M3([V; 3]);

impl M3 {
    fn euler_deg(r: V) -> M3 {
        let (sx, cx) = r[0].to_radians().sin_cos();
        let (sy, cy) = r[1].to_radians().sin_cos();
        let (sz, cz) = r[2].to_radians().sin_cos();
        // Rz * Ry * Rx
        M3([
            [cz * cy, cz * sy * sx - sz * cx, cz * sy * cx + sz * sx],
            [sz * cy, sz * sy * sx + cz * cx, sz * sy * cx - cz * sx],
            [-sy, cy * sx, cy * cx],
        ])
    }
    fn apply(&self, v: V) -> V {
        [dot(self.0[0], v), dot(self.0[1], v), dot(self.0[2], v)]
    }
    /// The inverse of a rotation is its transpose.
    fn apply_t(&self, v: V) -> V {
        let m = &self.0;
        [
            m[0][0] * v[0] + m[1][0] * v[1] + m[2][0] * v[2],
            m[0][1] * v[0] + m[1][1] * v[1] + m[2][1] * v[2],
            m[0][2] * v[0] + m[1][2] * v[1] + m[2][2] * v[2],
        ]
    }
}

// ------------------------------------------------------------------ colours

/// "#d9730d" or a name → 0..1 sRGB. Unknown colours are mid grey.
fn colour(hex: &str) -> V {
    let h = hex.trim().trim_start_matches('#');
    let named = match hex.trim().to_lowercase().as_str() {
        "red" => Some("d23c32"),
        "orange" => Some("d9730d"),
        "yellow" => Some("e8c547"),
        "green" => Some("4f9a55"),
        "blue" => Some("3a6fc4"),
        "purple" => Some("7b52ab"),
        "pink" => Some("e07aa5"),
        "brown" => Some("8a5a3b"),
        "white" => Some("f4f4f4"),
        "black" => Some("202020"),
        "grey" | "gray" => Some("9a9a9a"),
        "gold" => Some("d4a52c"),
        "silver" => Some("c0c4c8"),
        _ => None,
    };
    let h = named.unwrap_or(h);
    if h.len() == 6 {
        if let Ok(v) = u32::from_str_radix(h, 16) {
            return [((v >> 16) & 255) as f64 / 255.0, ((v >> 8) & 255) as f64 / 255.0, (v & 255) as f64 / 255.0];
        }
    }
    [0.6, 0.6, 0.6]
}

/// A colour as light: sRGB undone, so shading adds and scales it the way
/// light does. `lin:r,g,b` (what an animated colour is written as) is taken
/// as already linear.
fn linear(s: &str) -> V {
    if let Some(rest) = s.strip_prefix("lin:") {
        let f: Vec<f64> = rest.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        if f.len() == 3 {
            return [f[0], f[1], f[2]];
        }
    }
    let c = colour(s);
    [c[0].powf(2.2), c[1].powf(2.2), c[2].powf(2.2)]
}

/// Narkowicz's fit of the ACES filmic curve, then the display's gamma.
fn screen_byte(v: f64) -> u8 {
    let x = v.max(0.0) * 0.85;
    let t = (x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14);
    (t.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0 + 0.5) as u8
}

// --------------------------------------------------------- hitting shapes

struct Hit {
    t: f64,
    /// Outward, as the shape faces (not turned toward the ray).
    normal: V,
    albedo: V,
    shine: f64,
    /// Glass: its index of refraction.
    glass: Option<f64>,
    /// Glow: light given off, times `albedo`.
    glow: f64,
    /// A glowing thing that surfaces aim at directly (it has a bound).
    aimed: bool,
}

/// A pattern made ready: which, the second colour (as light), and its size.
#[derive(Clone, Copy)]
struct Painted {
    kind: PatternKind,
    other: V,
    size: f64,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum PatternKind {
    Checker,
    Stripes,
    Grid,
    Dots,
    Noise,
}

fn pattern_kind(name: &str) -> Option<PatternKind> {
    Some(match name.trim().to_lowercase().as_str() {
        "checker" | "checkers" | "checkerboard" | "chequer" => PatternKind::Checker,
        "stripes" | "stripe" | "stripy" | "bands" => PatternKind::Stripes,
        "grid" | "lines" | "tiles" => PatternKind::Grid,
        "dots" | "spots" | "polka" | "polka dots" => PatternKind::Dots,
        "noise" | "marble" | "mottled" | "stone" => PatternKind::Noise,
        _ => return None,
    })
}

fn painted(o: &Object) -> Option<Painted> {
    let (kind, colour, size) = match o.pattern.as_ref()? {
        Pattern::Kind(k) => (k.as_str(), None, None),
        Pattern::Full { kind, colour, size } => (kind.as_str(), colour.as_deref(), *size),
    };
    let kind = pattern_kind(kind)?;
    let base = linear(&o.colour);
    // No second colour: a darker version of the object's own.
    let other = colour.map(linear).unwrap_or_else(|| mul(base, 0.35));
    let size = size.filter(|s| s.is_finite() && *s > 1e-4).unwrap_or(0.25);
    Some(Painted { kind, other, size })
}

/// How much of the second colour at `q` (object space): 0 or 1, or between
/// for noise.
fn pattern_at(p: &Painted, q: V) -> f64 {
    let g = mul(q, 1.0 / p.size);
    match p.kind {
        PatternKind::Checker => ((g[0].floor() + g[1].floor() + g[2].floor()) as i64).rem_euclid(2) as f64,
        PatternKind::Stripes => (g[1].floor() as i64).rem_euclid(2) as f64,
        PatternKind::Grid => {
            let edge = |x: f64| {
                let f = x - x.floor();
                f.min(1.0 - f)
            };
            if edge(g[0]).min(edge(g[1])).min(edge(g[2])) < 0.06 { 1.0 } else { 0.0 }
        }
        PatternKind::Dots => {
            let c = [g[0] - g[0].floor() - 0.5, g[1] - g[1].floor() - 0.5, g[2] - g[2].floor() - 0.5];
            if dot(c, c) < 0.3 * 0.3 { 1.0 } else { 0.0 }
        }
        PatternKind::Noise => {
            // Three octaves of smooth value noise.
            let mut v = 0.0;
            let mut amp = 0.5;
            let mut f = 1.0;
            for o in 0..3 {
                v += amp * value_noise(mul(g, f), o);
                amp *= 0.5;
                f *= 2.0;
            }
            ((v / 0.875 - 0.5) * 1.6 + 0.5).clamp(0.0, 1.0)
        }
    }
}

/// Smooth value noise in 0..1: random values at whole-number points,
/// blended with a smoothstep.
fn value_noise(p: V, octave: u64) -> f64 {
    let i = [p[0].floor(), p[1].floor(), p[2].floor()];
    let f = [p[0] - i[0], p[1] - i[1], p[2] - i[2]];
    let s = f.map(|x| x * x * (3.0 - 2.0 * x));
    let at = |dx: f64, dy: f64, dz: f64| {
        let k = |x: f64| (x as i64) as u64;
        rnd(k(i[0] + dx) ^ (octave << 40), k(i[1] + dy), k(i[2] + dz) ^ 0x5eed)
    };
    let lerp = |a: f64, b: f64, t: f64| a + (b - a) * t;
    let x00 = lerp(at(0.0, 0.0, 0.0), at(1.0, 0.0, 0.0), s[0]);
    let x10 = lerp(at(0.0, 1.0, 0.0), at(1.0, 1.0, 0.0), s[0]);
    let x01 = lerp(at(0.0, 0.0, 1.0), at(1.0, 0.0, 1.0), s[0]);
    let x11 = lerp(at(0.0, 1.0, 1.0), at(1.0, 1.0, 1.0), s[0]);
    lerp(lerp(x00, x10, s[1]), lerp(x01, x11, s[1]), s[2])
}

/// A model made ready: the mesh, and how its own coordinates become the
/// object's (take `centre` off, then times `k`): centred over `at`, standing
/// on it, sized to `fit`.
struct Fitted {
    mesh: std::sync::Arc<crate::meshio::Mesh>,
    centre: V,
    k: f64,
}

fn fitted(file: &str, fit: Option<f64>, base: Option<&std::path::Path>) -> Result<Fitted, String> {
    let mesh = crate::meshio::cached(&model_path(file, base))?;
    let (lo, hi) = (mesh.lo, mesh.hi);
    let centre = [(lo[0] + hi[0]) / 2.0, lo[1], (lo[2] + hi[2]) / 2.0];
    let longest = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(hi[2] - lo[2]);
    let k = match fit {
        Some(f) if f.is_finite() && f > 0.0 && longest > 1e-12 => f / longest,
        _ => 1.0,
    };
    Ok(Fitted { mesh, centre, k })
}

/// A model's file: as given when absolute, else beside the scene.
fn model_path(file: &str, base: Option<&std::path::Path>) -> std::path::PathBuf {
    let p = std::path::Path::new(file);
    match base {
        Some(b) if p.is_relative() => b.join(p),
        _ => p.to_path_buf(),
    }
}

/// An object made ready to be hit: its transform and colour worked out once.
struct Placed {
    shape: Shape,
    at: V,
    rot: M3,
    scale: f64,
    albedo: V,
    shine: f64,
    glass: Option<f64>,
    glow: f64,
    pattern: Option<Painted>,
    model: Option<Fitted>,
    /// A sphere around it, for a quick miss.
    bound: Option<(V, f64)>,
}

fn place(o: &Object, base: Option<&std::path::Path>) -> Placed {
    let scale = if o.scale.is_finite() && o.scale > 1e-6 { o.scale } else { 1.0 };
    let rot = M3::euler_deg(o.rotate);
    // A model that won't read is left out of the picture (check_scene says
    // why before drawing).
    let model = match &o.shape {
        Shape::Mesh { file, fit } => fitted(file, *fit, base).ok(),
        _ => None,
    };
    let local_bound: Option<(V, f64)> = match &o.shape {
        Shape::Mesh { .. } => model.as_ref().map(|m| {
            let (lo, hi) = (m.mesh.lo, m.mesh.hi);
            let mid = mul(add(lo, hi), 0.5);
            (mul(sub(mid, m.centre), m.k), len(sub(hi, lo)) / 2.0 * m.k * 1.0001)
        }),
        Shape::Sphere { radius } => Some(([0.0; 3], *radius)),
        Shape::Box { size } => Some(([0.0; 3], len(*size) / 2.0)),
        Shape::Cylinder { radius, height } | Shape::Cone { radius, height } | Shape::Capsule { radius, height } => {
            Some(([0.0, height / 2.0, 0.0], (radius * radius + height * height / 4.0).sqrt()))
        }
        Shape::Torus { radius, thickness } => Some(([0.0; 3], radius + thickness)),
        Shape::Ground { .. } => None,
    };
    let glass = (o.material == Material::Glass).then(|| o.ior.filter(|i| i.is_finite()).unwrap_or(1.5).clamp(1.0, 3.0));
    // Glass left at the default grey would be smoked glass; plain glass is clear.
    let albedo = if glass.is_some() && o.colour == grey() { [0.97; 3] } else { linear(&o.colour) };
    Placed {
        shape: o.shape.clone(),
        at: o.at,
        rot,
        scale,
        albedo,
        shine: o.shine.clamp(0.0, 1.0),
        glass,
        glow: if o.material == Material::Glow { o.glow.filter(|g| g.is_finite()).unwrap_or(3.0).clamp(0.0, 100.0) } else { 0.0 },
        pattern: painted(o),
        bound: local_bound.map(|(c, r)| (add(o.at, mul(rot.apply(c), scale)), r * scale)),
        model,
    }
}

const EPS: f64 = 1e-6;

/// The nearest t > EPS where the ray (o, d) meets a sphere at `c`, radius `r`.
fn sphere_t(o: V, d: V, c: V, r: f64) -> Option<f64> {
    let oc = sub(o, c);
    let b = dot(oc, d);
    let cc = dot(oc, oc) - r * r;
    let disc = b * b - cc;
    if disc < 0.0 {
        return None;
    }
    let sq = disc.sqrt();
    [-b - sq, -b + sq].into_iter().find(|t| *t > EPS)
}

/// Where a ray hits a shape in the shape's own space: (t, normal).
fn hit_local(shape: &Shape, o: V, d: V) -> Option<(f64, V)> {
    match shape {
        Shape::Sphere { radius } => sphere_t(o, d, [0.0; 3], *radius).map(|t| (t, unit(add(o, mul(d, t))))),
        Shape::Box { size } => {
            let hi = mul(*size, 0.5);
            let lo = mul(hi, -1.0);
            let (mut tmin, mut tmax) = (f64::MIN, f64::MAX);
            let (mut axis_in, mut axis_out) = (0, 0);
            for i in 0..3 {
                if d[i].abs() < 1e-12 {
                    if o[i] < lo[i] || o[i] > hi[i] {
                        return None;
                    }
                    continue;
                }
                let (mut t0, mut t1) = ((lo[i] - o[i]) / d[i], (hi[i] - o[i]) / d[i]);
                if t0 > t1 {
                    std::mem::swap(&mut t0, &mut t1);
                }
                if t0 > tmin {
                    tmin = t0;
                    axis_in = i;
                }
                if t1 < tmax {
                    tmax = t1;
                    axis_out = i;
                }
            }
            if tmin > tmax || tmax < EPS {
                return None;
            }
            let (t, axis, sign) = if tmin > EPS { (tmin, axis_in, -d[axis_in].signum()) } else { (tmax, axis_out, d[axis_out].signum()) };
            let mut n = [0.0; 3];
            n[axis] = sign;
            Some((t, n))
        }
        Shape::Cylinder { radius, height } => {
            let mut best: Option<(f64, V)> = None;
            let mut keep = |t: f64, n: V| {
                if t > EPS && best.map(|b| t < b.0).unwrap_or(true) {
                    best = Some((t, n));
                }
            };
            let a = d[0] * d[0] + d[2] * d[2];
            if a > 1e-12 {
                let b = o[0] * d[0] + o[2] * d[2];
                let c = o[0] * o[0] + o[2] * o[2] - radius * radius;
                let disc = b * b - a * c;
                if disc >= 0.0 {
                    for t in [(-b - disc.sqrt()) / a, (-b + disc.sqrt()) / a] {
                        let y = o[1] + d[1] * t;
                        if y >= 0.0 && y <= *height {
                            let p = add(o, mul(d, t));
                            keep(t, unit([p[0], 0.0, p[2]]));
                        }
                    }
                }
            }
            if d[1].abs() > 1e-12 {
                for (y, ny) in [(0.0, -1.0), (*height, 1.0)] {
                    let t = (y - o[1]) / d[1];
                    let p = add(o, mul(d, t));
                    if p[0] * p[0] + p[2] * p[2] <= radius * radius {
                        keep(t, [0.0, ny, 0.0]);
                    }
                }
            }
            best
        }
        Shape::Cone { radius, height } => {
            let mut best: Option<(f64, V)> = None;
            let mut keep = |t: f64, n: V| {
                if t > EPS && best.map(|b| t < b.0).unwrap_or(true) {
                    best = Some((t, n));
                }
            };
            // x² + z² = k²(h − y)²
            let k = radius / height.max(1e-9);
            let k2 = k * k;
            let hy = height - o[1];
            let a = d[0] * d[0] + d[2] * d[2] - k2 * d[1] * d[1];
            let b = o[0] * d[0] + o[2] * d[2] + k2 * hy * d[1];
            let c = o[0] * o[0] + o[2] * o[2] - k2 * hy * hy;
            let roots: Vec<f64> = if a.abs() > 1e-12 {
                let disc = b * b - a * c;
                if disc >= 0.0 {
                    vec![(-b - disc.sqrt()) / a, (-b + disc.sqrt()) / a]
                } else {
                    vec![]
                }
            } else if b.abs() > 1e-12 {
                vec![-c / (2.0 * b)]
            } else {
                vec![]
            };
            for t in roots {
                let p = add(o, mul(d, t));
                if p[1] >= 0.0 && p[1] <= *height {
                    let r_here = (p[0] * p[0] + p[2] * p[2]).sqrt().max(1e-9);
                    keep(t, unit([p[0] / r_here, k, p[2] / r_here]));
                }
            }
            if d[1].abs() > 1e-12 {
                let t = -o[1] / d[1];
                let p = add(o, mul(d, t));
                if p[0] * p[0] + p[2] * p[2] <= radius * radius {
                    keep(t, [0.0, -1.0, 0.0]);
                }
            }
            best
        }
        Shape::Capsule { radius, height } => {
            let r = *radius;
            let (y0, y1) = (r, (height - r).max(r));
            let mut best: Option<(f64, V)> = None;
            let mut keep = |t: f64, n: V| {
                if t > EPS && best.map(|b| t < b.0).unwrap_or(true) {
                    best = Some((t, n));
                }
            };
            let a = d[0] * d[0] + d[2] * d[2];
            if a > 1e-12 {
                let b = o[0] * d[0] + o[2] * d[2];
                let c = o[0] * o[0] + o[2] * o[2] - r * r;
                let disc = b * b - a * c;
                if disc >= 0.0 {
                    for t in [(-b - disc.sqrt()) / a, (-b + disc.sqrt()) / a] {
                        let p = add(o, mul(d, t));
                        if p[1] >= y0 && p[1] <= y1 {
                            keep(t, unit([p[0], 0.0, p[2]]));
                        }
                    }
                }
            }
            for cy in [y0, y1] {
                let c = [0.0, cy, 0.0];
                if let Some(t) = sphere_t(o, d, c, r) {
                    keep(t, unit(sub(add(o, mul(d, t)), c)));
                }
            }
            best
        }
        Shape::Torus { radius, thickness } => {
            // Sphere tracing the distance to the ring, inside its bound.
            let sdf = |p: V| {
                let q = ((p[0] * p[0] + p[2] * p[2]).sqrt() - radius, p[1]);
                (q.0 * q.0 + q.1 * q.1).sqrt() - thickness
            };
            let reach = radius + thickness;
            let t0 = sphere_t(o, d, [0.0; 3], reach).map(|t| {
                // Start at the bound's near side (or here, if inside).
                let oc = o;
                if dot(oc, oc) <= reach * reach {
                    0.0
                } else {
                    t
                }
            })?;
            let mut t = t0;
            let limit = t0 + 2.0 * reach + 1e-6;
            for _ in 0..160 {
                let p = add(o, mul(d, t));
                let dist = sdf(p);
                if dist.abs() < 1e-5 * reach.max(1.0) {
                    if t <= EPS {
                        break;
                    }
                    let e = 1e-4 * reach.max(1.0);
                    let n = unit([
                        sdf(add(p, [e, 0.0, 0.0])) - sdf(sub(p, [e, 0.0, 0.0])),
                        sdf(add(p, [0.0, e, 0.0])) - sdf(sub(p, [0.0, e, 0.0])),
                        sdf(add(p, [0.0, 0.0, e])) - sdf(sub(p, [0.0, 0.0, e])),
                    ]);
                    return Some((t, n));
                }
                t += dist.max(1e-5);
                if t > limit {
                    break;
                }
            }
            None
        }
        Shape::Ground { .. } | Shape::Mesh { .. } => None,
    }
}

fn hit_placed(p: &Placed, o: V, d: V, far: f64) -> Option<Hit> {
    if let Shape::Ground { y, pattern } = &p.shape {
        if d[1].abs() < 1e-12 {
            return None;
        }
        let t = (y - o[1]) / d[1];
        if t <= EPS || t >= far {
            return None;
        }
        let q = add(o, mul(d, t));
        let checker = pattern.as_deref().unwrap_or("checker") == "checker";
        let k = if checker && ((q[0].floor() + q[2].floor()) as i64).rem_euclid(2) == 1 { 0.78 } else { 1.0 };
        let albedo = match &p.pattern {
            // A pattern given to the ground replaces its checker; measured
            // from the ground's own height, as Blender's plane measures it.
            Some(pt) => mix(p.albedo, pt.other, pattern_at(pt, [q[0], q[1] - y, q[2]])),
            None => mul(p.albedo, k),
        };
        return Some(Hit { t, normal: [0.0, 1.0, 0.0], albedo, shine: p.shine, glass: p.glass, glow: p.glow, aimed: false });
    }
    if matches!(p.shape, Shape::Mesh { .. }) && p.model.is_none() {
        return None;
    }
    if let Some((c, r)) = p.bound {
        match sphere_t(o, d, c, r) {
            None if dot(sub(o, c), sub(o, c)) > r * r => return None,
            Some(t) if t >= far && dot(sub(o, c), sub(o, c)) > r * r => return None,
            _ => {}
        }
    }
    let ol = mul(p.rot.apply_t(sub(o, p.at)), 1.0 / p.scale);
    let dl = p.rot.apply_t(d);
    let (tl, nl, own) = match &p.model {
        Some(m) => {
            // Into the file's own coordinates; t there is t here over k.
            let om = add(mul(ol, 1.0 / m.k), m.centre);
            let h = m.mesh.hit(om, dl, far / (p.scale * m.k))?;
            (h.t * m.k, h.normal, h.colour.and_then(|i| m.mesh.colours.get(i).copied()))
        }
        None => {
            let (tl, nl) = hit_local(&p.shape, ol, dl)?;
            (tl, nl, None)
        }
    };
    let t = tl * p.scale;
    if t >= far {
        return None;
    }
    // The file's colours stand in for the object's own.
    let mut albedo = own.unwrap_or(p.albedo);
    if let Some(pt) = &p.pattern {
        // Measured a hair inside the surface: a face lying exactly on a
        // square's edge (a 0.8 box in 0.2 squares) would otherwise flicker
        // between the two colours from rounding.
        let q = sub(add(ol, mul(dl, tl)), mul(unit(nl), 1e-6 + pt.size * 1e-4));
        albedo = mix(albedo, pt.other, pattern_at(pt, q));
    }
    Some(Hit { t, normal: unit(p.rot.apply(nl)), albedo, shine: p.shine, glass: p.glass, glow: p.glow, aimed: p.glow > 0.0 && p.bound.is_some() })
}

fn mix(a: V, b: V, k: f64) -> V {
    add(mul(a, 1.0 - k), mul(b, k))
}

fn nearest(world: &[Placed], o: V, d: V) -> Option<Hit> {
    let mut best: Option<Hit> = None;
    for p in world {
        let far = best.as_ref().map(|b| b.t).unwrap_or(f64::MAX);
        if let Some(h) = hit_placed(p, o, d, far) {
            best = Some(h);
        }
    }
    best
}

/// How much sunlight gets from `o` along `d`: all of it, none (something
/// solid in the way), or tinted and dimmed by each glass thing it passes.
/// (Glass doesn't focus light into caustics here; it only lets it through.)
fn transmit(world: &[Placed], o: V, d: V) -> V {
    let mut through = [1.0; 3];
    for p in world {
        if let Some(h) = hit_placed(p, o, d, f64::MAX) {
            match h.glass {
                None => return [0.0; 3],
                Some(_) => through = had(through, mul(h.albedo, 0.85)),
            }
        }
    }
    through
}

// ----------------------------------------------------------------- lighting

/// A fixed, well-spread random number for (pixel, sample, which): the same
/// every frame, so still parts of a moving scene keep the same grain.
fn rnd(a: u64, b: u64, c: u64) -> f64 {
    let mut x = a.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ b.wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ c.wrapping_mul(0x1656_67B1_9E37_79F9);
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    (x >> 11) as f64 / (1u64 << 53) as f64
}

/// Two numbers in 0..1 for this pixel's `s`-th ray, for lighting sample
/// `which`: Roberts' R2 low-discrepancy sequence, shifted by a fixed random
/// amount per pixel (Cranley–Patterson rotation). The rays of one pixel then
/// spread evenly over the sun's disc and the sky instead of clumping, which
/// is most of the grain gone at the same cost.
fn spread(px: u64, s: u64, which: u64) -> (f64, f64) {
    const A1: f64 = 0.754_877_666_246_692_7;
    const A2: f64 = 0.569_840_290_998_053_2;
    let (o1, o2) = (rnd(px, which, 101), rnd(px, which, 102));
    (((s as f64 + 1.0) * A1 + o1).fract(), ((s as f64 + 1.0) * A2 + o2).fract())
}

/// Two perpendicular directions to `n`.
fn basis(n: V) -> (V, V) {
    let a = if n[0].abs() > 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
    let u = unit(cross(n, a));
    (u, cross(n, u))
}

struct Light {
    dir: V,
    colour: V,
    /// Cosine of the sun's angular radius.
    cone: f64,
    sky_h: V,
    sky_top: V,
}

fn sky(l: &Light, d: V) -> V {
    // A glow toward the sun.
    let s = dot(d, l.dir).max(0.0).powf(64.0) * 0.5;
    add(sky_dome(l, d), mul(l.colour, s))
}

/// The sky without the sun's glow: what lights things from all round. (With
/// the glow in, one ray in a hundred that happened to point at the sun lit a
/// whole pixel — speckle on every upward face.)
fn sky_dome(l: &Light, d: V) -> V {
    let up = d[1].clamp(-1.0, 1.0);
    let k = up.max(0.0).powf(0.6);
    let mut c = add(mul(l.sky_h, 1.0 - k), mul(l.sky_top, k));
    if up < 0.0 {
        c = mul(l.sky_h, 0.85);
    }
    c
}

/// How deep glass may send rays on (in and out of a glass ball is four).
const GLASS_DEPTH: u32 = 8;

fn shade(world: &[Placed], l: &Light, o: V, d: V, depth: u32, seed: (u64, u64)) -> V {
    shade_hit(world, l, o, d, nearest(world, o, d), depth, seed)
}

/// `shade` for a ray whose first hit is already known.
fn shade_hit(world: &[Placed], l: &Light, o: V, d: V, first: Option<Hit>, depth: u32, seed: (u64, u64)) -> V {
    let Some(h) = first else { return sky(l, d) };
    let p = add(o, mul(d, h.t));
    let n = if dot(h.normal, d) > 0.0 { mul(h.normal, -1.0) } else { h.normal };
    let lift = add(p, mul(n, 1e-4 * (1.0 + h.t)));
    let (px, sm) = seed;
    let salt = depth as u64 * 7;
    let haze = 1.0 - (-h.t * 0.012).exp();
    let hazed = |c: V| add(mul(c, 1.0 - haze), mul(l.sky_h, haze));

    if h.glow > 0.0 {
        return hazed(mul(h.albedo, h.glow));
    }
    if let Some(ior) = h.glass {
        if depth >= GLASS_DEPTH {
            return hazed(sky_dome(l, d));
        }
        // Going in when the ray meets the outside of the surface.
        let entering = dot(h.normal, d) < 0.0;
        let eta = if entering { 1.0 / ior } else { ior };
        let cos_i = (-dot(d, n)).clamp(0.0, 1.0);
        let k = 1.0 - eta * eta * (1.0 - cos_i * cos_i);
        let r0 = ((1.0 - ior) / (1.0 + ior)).powi(2);
        let refl = unit(sub(d, mul(n, 2.0 * dot(d, n))));
        let below = sub(p, mul(n, 1e-4 * (1.0 + h.t)));
        let fres = if k < 0.0 {
            1.0 // all of it reflects: total internal reflection
        } else {
            let c = if entering { cos_i } else { k.sqrt() };
            r0 + (1.0 - r0) * (1.0 - c).powi(5)
        };
        let reflected = || shade(world, l, lift, refl, depth + 1, seed);
        let refracted = || {
            let t = unit(add(mul(d, eta), mul(n, eta * cos_i - k.sqrt())));
            let seen = shade(world, l, below, t, depth + 1, seed);
            if entering { had(seen, h.albedo) } else { seen }
        };
        // Both ways near the camera; further in, one way picked by the odds,
        // so a pile of glass doesn't cost 2^depth rays.
        let c = if k < 0.0 {
            reflected()
        } else if depth < 2 {
            add(mul(reflected(), fres), mul(refracted(), 1.0 - fres))
        } else if rnd(px, sm, 200 + depth as u64) < fres {
            reflected()
        } else {
            refracted()
        };
        // The sun's glint on the glass.
        let half = unit(sub(l.dir, d));
        let glint = if entering { dot(n, half).max(0.0).powf(400.0) * 0.8 } else { 0.0 };
        return hazed(add(c, mul(l.colour, glint)));
    }

    // The sun: a direction picked inside its disc, so shadow edges soften.
    let (u, v) = basis(l.dir);
    let (q1, q2) = spread(px, sm, 1 + salt);
    let r = (1.0 - l.cone) * q1;
    let cos_a = 1.0 - r;
    let sin_a = (1.0 - cos_a * cos_a).max(0.0).sqrt();
    let phi = std::f64::consts::TAU * q2;
    let to_sun = unit(add(mul(l.dir, cos_a), add(mul(u, sin_a * phi.cos()), mul(v, sin_a * phi.sin()))));
    let lambert = dot(n, to_sun).max(0.0);
    let through = if lambert > 0.0 { transmit(world, lift, to_sun) } else { [0.0; 3] };
    let lit = through != [0.0; 3];
    let direct = if lit { mul(had(l.colour, through), lambert) } else { [0.0; 3] };

    // The sky: light from a random direction over the surface (cosine
    // weighted), dimmed where something close is in the way.
    let (tu, tv) = basis(n);
    let (r1, r2) = spread(px, sm, 3 + salt);
    let sr = r1.sqrt();
    let ph = std::f64::consts::TAU * r2;
    let hemi = unit(add(mul(n, (1.0 - r1).sqrt()), add(mul(tu, sr * ph.cos()), mul(tv, sr * ph.sin()))));
    // Where the sky is blocked nearby, what's in the way lights it instead —
    // dimly, in its own colour (one bounce, roughly: the ground under a ball
    // throws its colour up onto it).
    // Glowing things light what's round them. Each is aimed at directly — a
    // direction picked inside the cone it fills as seen from here (next-event
    // estimation), weighted by that cone's size — rather than waiting for a
    // sky ray to find it, which left fireflies all over the floor.
    let mut lamps = [0.0; 3];
    for (gi, g) in world.iter().enumerate() {
        let (true, Some((gc, gr))) = (g.glow > 0.0, g.bound) else { continue };
        let to = sub(gc, lift);
        let d2 = dot(to, to);
        if d2 <= gr * gr {
            continue;
        }
        let cos_max = (1.0 - gr * gr / d2).sqrt();
        let omega = std::f64::consts::TAU * (1.0 - cos_max);
        let (q1, q2) = spread(px, sm, 50 + 3 * gi as u64 + salt);
        let cos_t = 1.0 - q1 * (1.0 - cos_max);
        let sin_t = (1.0 - cos_t * cos_t).max(0.0).sqrt();
        let phi = std::f64::consts::TAU * q2;
        let axis = unit(to);
        let (a1, a2) = basis(axis);
        let dir = unit(add(mul(axis, cos_t), add(mul(a1, sin_t * phi.cos()), mul(a2, sin_t * phi.sin()))));
        let lam = dot(n, dir);
        if lam <= 0.0 {
            continue;
        }
        let Some(mine) = hit_placed(g, lift, dir, f64::MAX) else { continue };
        let first = nearest(world, lift, dir).map(|b| b.t).unwrap_or(f64::MAX);
        if first + 1e-9 >= mine.t {
            lamps = add(lamps, mul(mine.albedo, mine.glow * lam * omega / std::f64::consts::PI));
        }
    }
    // A sky ray that finds a lamp adds nothing (the lamps were counted just
    // above); glass lets most of the sky through.
    let ambient = match nearest(world, lift, hemi) {
        Some(b) if b.aimed => [0.0; 3],
        Some(b) if b.glow > 0.0 => mul(b.albedo, b.glow / 0.6),
        Some(b) if b.glass.is_some() => mul(sky_dome(l, hemi), 0.8),
        Some(b) if b.t < 2.5 => had(b.albedo, add(mul(l.sky_h, 0.35), mul(l.colour, 0.12))),
        _ => sky_dome(l, hemi),
    };

    let mut c = had(h.albedo, add(add(direct, lamps), mul(ambient, 0.6)));

    // A highlight and, on shiny things, what they reflect — more at a glance.
    let half = unit(sub(to_sun, d));
    if lit {
        let spec = dot(n, half).max(0.0).powf(16.0 + 240.0 * h.shine) * (0.03 + 0.25 * h.shine);
        c = add(c, mul(l.colour, spec));
    }
    if h.shine > 0.0 && depth < 2 {
        let cos = (-dot(d, n)).clamp(0.0, 1.0);
        let f0 = 0.04 + 0.3 * h.shine * h.shine;
        let fres = f0 + (1.0 - f0) * (1.0 - cos).powi(5);
        let refl = unit(sub(d, mul(n, 2.0 * dot(d, n))));
        let seen = shade(world, l, lift, refl, depth + 1, seed);
        // Shiny things tint what they reflect toward their own colour, as
        // metals do — so gold stays gold instead of mirroring a white floor.
        let top = h.albedo[0].max(h.albedo[1]).max(h.albedo[2]).max(1e-6);
        let tint = add(mul([1.0; 3], 1.0 - 0.8 * h.shine), mul(h.albedo, 0.8 * h.shine / top));
        c = add(mul(c, 1.0 - fres * h.shine), mul(had(seen, tint), fres * h.shine));
    }

    // Haze: far things fade into the horizon.
    hazed(c)
}

fn light_of(scene: &Scene) -> Light {
    let (from, softness, col, strength) = match &scene.sun {
        Sun::From(v) => (*v, default_softness(), default_sun_colour(), 1.0),
        Sun::Full { from, softness, colour, strength } => (*from, *softness, colour.clone(), *strength),
    };
    let sky_h = linear(&scene.sky);
    let sky_top = scene.sky_top.as_deref().map(linear).unwrap_or_else(|| mul(sky_h, 0.72));
    Light {
        dir: unit(from),
        colour: mul(linear(&col), 2.0 * strength.clamp(0.0, 10.0)),
        cone: (softness.clamp(0.0, 45.0) / 2.0).to_radians().cos(),
        sky_h,
        sky_top,
    }
}

/// The camera's frame: forward, right, up, and the half-height of the view.
fn camera_frame(c: &Camera) -> (V, V, V, f64) {
    let fwd = unit(sub(c.at, c.from));
    let right = unit(cross(fwd, [0.0, 1.0, 0.0]));
    let right = if dot(right, right) < 0.5 { [1.0, 0.0, 0.0] } else { right };
    let up = cross(right, fwd);
    (fwd, right, up, (c.fov.clamp(5.0, 150.0).to_radians() / 2.0).tan())
}

/// Where a point lands on the picture at time `t`, in pixels, if in front of
/// the camera.
fn project(scene: &Scene, t: f64, p: V) -> Option<(f64, f64)> {
    let c = camera_at(&scene.camera, scene.duration, t);
    let (fwd, right, up, half_h) = camera_frame(&c);
    let rel = sub(p, c.from);
    let z = dot(rel, fwd);
    if z <= 1e-9 {
        return None;
    }
    let (w, h) = (scene.width as f64, scene.height as f64);
    let half_w = half_h * w / h;
    let x = dot(rel, right) / z / half_w;
    let y = dot(rel, up) / z / half_h;
    Some(((x + 1.0) / 2.0 * w, (1.0 - y) / 2.0 * h))
}

/// One frame at time `t`.
/// Relative standard error past which a pixel gets more rays (adaptive).
///
/// Measured in round 10 against a 64-ray reference, sweeping 0.02 to 0.5.
/// At 0.08, on round 8's night-and-lamp scene, a draft spends 5.3 rays a
/// pixel and lands 2.64 levels off, where uniform sampling at that cost
/// would be about 2.85: some 7% better. On a held-out
/// daylight scene with a checker floor and glass it loses outright (9.3
/// rays a pixel, 4.7 levels off, against 3.4 for 9 uniform rays): a distant
/// checker's pixels are wrong while their few rays *agree*, so nothing
/// flags them. Hence off unless asked for.
const ADAPT_REL: f64 = 0.08;

/// Rays per pixel in the last frame drawn, times 1000: what adaptive
/// sampling actually spent, for saying so and for measuring it.
pub static LAST_RAYS_PER_PIXEL_X1000: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn render_frame(scene: &Scene, t: f64) -> Rgba {
    let (w, h) = (scene.width.clamp(1, 4096), scene.height.clamp(1, 4096));
    let side = scene.quality.side();
    let n = side * side;
    let light = light_of(scene);
    // Moments inside the open shutter; each ray takes one, so motion blurs.
    let slices: Vec<f64> = if scene.duration > 0.0 && scene.motion_blur {
        let open = 0.5 / scene.fps.clamp(1, 60) as f64;
        (0..5).map(|i| t + open * (i as f64 / 4.0 - 0.5)).collect()
    } else {
        vec![t]
    };
    let states: Vec<(Vec<Placed>, (V, V, V, V, f64))> = slices
        .iter()
        .map(|&ti| {
            let s = scene_at(scene, ti.max(0.0));
            let world: Vec<Placed> = s.objects.iter().map(|o| place(o, scene.base.as_deref())).collect();
            let (fwd, right, up, half_h) = camera_frame(&s.camera);
            (world, (s.camera.from, fwd, right, up, half_h))
        })
        .collect();
    let aspect = w as f64 / h as f64;
    let adaptive = scene.adaptive.unwrap_or(false);
    let rays = std::sync::atomic::AtomicU64::new(0);
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 32);
    let rows_each = (h as usize).div_ceil(threads);
    // Each pixel: its light (linear), and for the denoiser what the middle
    // of the pixel first sees — its surface's normal, colour and distance —
    // and how much the pixel's rays disagreed.
    let bands: Vec<Vec<Texel>> = std::thread::scope(|sc| {
        let handles: Vec<_> = (0..threads)
            .map(|b| {
                let states = &states;
                let light = &light;
                let rays = &rays;
                sc.spawn(move || {
                    let y0 = b * rows_each;
                    let y1 = ((b + 1) * rows_each).min(h as usize);
                    let mut out = Vec::with_capacity((y1.saturating_sub(y0)) * w as usize);
                    for y in y0..y1 {
                        for x in 0..w as usize {
                            let px = (y as u64) << 32 | x as u64;
                            let mut acc = [0.0; 3];
                            let mut lum2 = 0.0;
                            let (mut normal, mut albedo, mut depth) = ([0.0; 3], [0.0; 3], 0.0);
                            let k = states.len();
                            // The first `n` rays are stratified over the pixel;
                            // extra rays (adaptive) land anywhere in it.
                            let mut taken = 0usize;
                            let mut s = 0usize;
                            let mut limit = n;
                            while s < limit {
                                let (jx, jy) = if s < n {
                                    let (sx, sy) = ((s % side) as f64, (s / side) as f64);
                                    ((sx + rnd(px, s as u64, 11)) / side as f64, (sy + rnd(px, s as u64, 12)) / side as f64)
                                } else {
                                    // Extra rays are stratified too, over a grid
                                    // twice as fine as the first rays'.
                                    let (f, e) = (side * 2, s - n);
                                    let (sx, sy) = ((e % f) as f64, (e / f) as f64);
                                    ((sx + rnd(px, s as u64, 11)) / f as f64, (sy + rnd(px, s as u64, 12)) / f as f64)
                                };
                                // A moment in the open shutter, spread evenly over the
                                // pixel's rays and jittered within its share.
                                let slot = if s < n {
                                    (((s as f64 + rnd(px, s as u64, 13)) / n as f64) * k as f64) as usize
                                } else {
                                    (rnd(px, s as u64, 13) * k as f64) as usize
                                };
                                let (world, (from, fwd, right, up, half_h)) = &states[slot.min(k - 1)];
                                let u = ((x as f64 + jx) / w as f64 * 2.0 - 1.0) * half_h * aspect;
                                let v = (1.0 - (y as f64 + jy) / h as f64 * 2.0) * half_h;
                                let d = unit(add(*fwd, add(mul(*right, u), mul(*up, v))));
                                let first = nearest(world, *from, d);
                                // What this ray first sees, for the denoiser: averaged
                                // over the pixel's rays like the light is, so dividing
                                // one by the other stays fair at edges.
                                let (fn_, fa, fd) = match &first {
                                    // Glass, mirrors and lamps show other things (or
                                    // themselves) rather than a lit colour: not divided out.
                                    Some(hh) if hh.glass.is_some() || hh.glow > 0.0 || hh.shine > 0.5 => (hh.normal, [1.0; 3], hh.t),
                                    Some(hh) => (hh.normal, hh.albedo.map(|a| a.max(0.02)), hh.t),
                                    None => ([0.0; 3], [1.0; 3], 1e4),
                                };
                                normal = add(normal, fn_);
                                albedo = add(albedo, fa);
                                depth += fd;
                                let c = shade_hit(world, light, *from, d, first, 0, (px, s as u64));
                                acc = add(acc, c);
                                lum2 += luma(c) * luma(c);
                                taken += 1;
                                s += 1;
                                // After the first rays: do they agree? Where the
                                // standard error of the mean is large next to the
                                // pixel's own brightness, take three times as many
                                // again. Judged once, so the cost is bounded.
                                if adaptive && s == n {
                                    let m = mul(acc, 1.0 / n as f64);
                                    let v = (lum2 / n as f64 - luma(m) * luma(m)).max(0.0) / n as f64;
                                    if v.sqrt() > ADAPT_REL * (luma(m) + 0.05) {
                                        limit = n * 5;
                                    }
                                }
                            }
                            rays.fetch_add(taken as u64, std::sync::atomic::Ordering::Relaxed);
                            let n = taken;
                            let c = mul(acc, 1.0 / n as f64);
                            let var = (lum2 / n as f64 - luma(c) * luma(c)).max(0.0);
                            let inv = 1.0 / n as f64;
                            let nl = len(normal);
                            let normal = if nl > 1e-9 { mul(normal, 1.0 / nl) } else { [0.0; 3] };
                            out.push(Texel { c, var: var * inv, normal, albedo: mul(albedo, inv), depth: depth * inv });
                        }
                    }
                    out
                })
            })
            .collect();
        handles.into_iter().map(|hd| hd.join().unwrap_or_default()).collect()
    });
    let mut texels: Vec<Texel> = bands.into_iter().flatten().collect();
    LAST_RAYS_PER_PIXEL_X1000.store(
        rays.load(std::sync::atomic::Ordering::Relaxed) * 1000 / (w as u64 * h as u64).max(1),
        std::sync::atomic::Ordering::Relaxed,
    );
    // Measured: it takes about a tenth of the error out of a draft, and
    // nothing out of "good" — so it's on by default for drafts only.
    let smooth = scene.denoise.unwrap_or(scene.quality == Quality::Draft);
    if smooth && texels.len() == (w * h) as usize {
        denoise(&mut texels, w as usize, h as usize);
    }
    let mut img = Rgba::new(w, h);
    for (i, t) in texels.iter().enumerate() {
        img.pixels[i * 4..i * 4 + 4].copy_from_slice(&[screen_byte(t.c[0]), screen_byte(t.c[1]), screen_byte(t.c[2]), 255]);
    }
    img
}

/// One pixel's light and what the denoiser steers by.
#[derive(Clone, Copy, Default)]
struct Texel {
    c: V,
    /// How uncertain `c` is: the variance of its mean (brightness).
    var: f64,
    normal: V,
    albedo: V,
    depth: f64,
}

fn luma(c: V) -> f64 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// Edge-avoiding à-trous wavelet filtering (Dammertz, Sewtz, Hanika and
/// Lensch, HPG 2010), with SVGF's idea of steering by each pixel's own noise
/// (Schied et al. 2017). Three passes of a 5×5 B3-spline kernel whose taps
/// spread 1, 2 and 4 pixels apart; a neighbour counts less the more its
/// normal, distance or light differs. The surface colour is divided out first
/// and put back after, so a pattern or texture is never blurred — only the
/// light on it.
fn denoise(px: &mut [Texel], w: usize, h: usize) {
    const KERNEL: [f64; 5] = [1.0 / 16.0, 1.0 / 4.0, 3.0 / 8.0, 1.0 / 4.0, 1.0 / 16.0];
    let mut light: Vec<V> = px.iter().map(|t| [t.c[0] / t.albedo[0], t.c[1] / t.albedo[1], t.c[2] / t.albedo[2]]).collect();
    // Noise of the light (divided the same way, roughly).
    let raw: Vec<f64> = px.iter().map(|t| t.var / luma(t.albedo).max(0.02).powi(2)).collect();
    // A few rays make a shaky guess at a pixel's noise (four rays that all
    // happened to agree say "none"), so it's averaged over the 3×3 round it
    // first, as SVGF does.
    let mut var = raw.clone();
    for y in 0..h {
        for x in 0..w {
            let (mut s, mut n) = (0.0, 0.0);
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let (xx, yy) = (x as i64 + dx, y as i64 + dy);
                    if xx >= 0 && yy >= 0 && xx < w as i64 && yy < h as i64 {
                        let k = if dx == 0 && dy == 0 { 4.0 } else if dx == 0 || dy == 0 { 2.0 } else { 1.0 };
                        s += k * raw[yy as usize * w + xx as usize];
                        n += k;
                    }
                }
            }
            var[y * w + x] = s / n;
        }
    }
    // How fast distance changes from pixel to pixel here: on a floor seen
    // at a low angle it changes a lot, and neighbours are still the same
    // surface (SVGF's depth test, scaled by the depth's slope).
    let slope: Vec<f64> = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let z = |xx: usize, yy: usize| px[yy * w + xx].depth;
            let gx = if x + 1 < w && x > 0 { (z(x + 1, y) - z(x - 1, y)).abs() / 2.0 } else { 0.0 };
            let gy = if y + 1 < h && y > 0 { (z(x, y + 1) - z(x, y - 1)).abs() / 2.0 } else { 0.0 };
            gx.max(gy).min(px[i].depth)
        })
        .collect();
    let mut next = light.clone();
    let mut next_var = var.clone();
    // Three passes and a tight light test were measured best (see
    // docs/live/round8): more passes smeared the steep fall-off round a lamp.
    for pass in 0..3 {
        let step = 1usize << pass;
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                let me = &px[i];
                let li = luma(light[i]);
                let sd = var[i].sqrt();
                let (mut sum, mut wsum, mut vsum) = ([0.0; 3], 0.0, 0.0);
                for (ky, hy) in KERNEL.iter().enumerate() {
                    let yy = y as i64 + (ky as i64 - 2) * step as i64;
                    if yy < 0 || yy >= h as i64 {
                        continue;
                    }
                    for (kx, hx) in KERNEL.iter().enumerate() {
                        let xx = x as i64 + (kx as i64 - 2) * step as i64;
                        if xx < 0 || xx >= w as i64 {
                            continue;
                        }
                        let j = yy as usize * w + xx as usize;
                        let them = &px[j];
                        let wn = dot(me.normal, them.normal).max(0.0).powi(64);
                        let wn = if me.normal == [0.0; 3] && them.normal == [0.0; 3] { 1.0 } else { wn };
                        let apart = (((kx as f64 - 2.0).powi(2) + (ky as f64 - 2.0).powi(2)).sqrt()) * step as f64;
                        let dz = (me.depth - them.depth).abs() / (slope[i] * apart + me.depth.min(1e6) * 0.005 + 1e-3);
                        let dl = (li - luma(light[j])).abs() / (1.5 * sd + 0.01);
                        let wt = hy * hx * wn * (-dz - dl).exp();
                        sum = add(sum, mul(light[j], wt));
                        vsum += wt * wt * var[j];
                        wsum += wt;
                    }
                }
                next[i] = mul(sum, 1.0 / wsum.max(1e-12));
                next_var[i] = vsum / (wsum * wsum).max(1e-24);
            }
        }
        std::mem::swap(&mut light, &mut next);
        std::mem::swap(&mut var, &mut next_var);
    }
    for (t, l) in px.iter_mut().zip(light) {
        t.c = had(l, t.albedo);
    }
}

/// Every frame of the scene, in order (one for a still scene).
fn render_animation(scene: &Scene) -> Vec<Rgba> {
    frame_times(scene).into_iter().map(|t| render_frame(scene, t)).collect()
}

/// The camera circling a still scene: `frames` pictures, one full turn.
fn turntable(scene: &Scene, frames: usize) -> Vec<Rgba> {
    let mut s = scene.clone();
    s.duration = frames.max(1) as f64 / 12.0;
    s.fps = 12;
    s.camera.orbit = 360.0;
    s.motion_blur = false;
    s.objects.iter_mut().for_each(|o| {
        o.animate.clear();
        o.spin = [0.0; 3];
    });
    render_animation(&s)
}

// ------------------------------------------------------------------ reading

/// The instruction handed to a model drafting a scene.
pub const SCENE_SYSTEM: &str = "\
You are describing a small 3-D scene for a personal assistant to draw, and \
animate when asked. Return ONLY JSON, in a single fenced code block:
{\"width\":480, \"height\":360, \"sky\":\"#dfe8f2\", \"sun\":{\"from\":[x,y,z], \
\"softness\":4}, \"camera\":{\"from\":[x,y,z], \"at\":[x,y,z], \"fov\":40, \
\"orbit\":0}, \"duration\":0, \"fps\":24, \"objects\":[...]}
y is up; units are metres. Each object has \"shape\" and its size: sphere \
{radius}, box {size:[w,h,d]}, cylinder/cone/capsule {radius, height} (standing \
on \"at\"), torus {radius, thickness} (lying flat), ground {y}. Every object may \
add \"at\":[x,y,z], \"rotate\":[deg x,y,z], \"scale\", \"colour\":\"#rrggbb\", \
\"shine\" 0..1, \"material\":\"solid\"|\"glass\"|\"glow\" (glass may add \"ior\"; \
glow may add \"glow\" brightness, 3 by default), \"pattern\":\"checker\"|\"stripes\"|\
\"grid\"|\"dots\"|\"noise\" or {\"kind\", \"colour\", \"size\"}. A model file is \
{\"shape\":\"mesh\", \"file\":\"name.glb\", \"fit\": metres} — only files you are told \
exist. To make it move, set \"duration\" in seconds and give objects \
\"spin\":[deg/s x,y,z] or \"animate\":[{\"prop\":\"at\"|\"rotate\"|\"scale\"|\"colour\", \
\"keys\":[[time, value], ...], \"ease\":\"linear\"|\"ease-in-out\"|\"ease-in\"|\
\"ease-out\"|\"bounce\"|\"back\"|\"elastic\"|\"step\", \"loop\":false}]; the camera \
may \"orbit\" (degrees over the duration) or animate \"from\"/\"at\"/\"fov\". Keep \
every object in view for the whole animation. No explanation.";

/// The model files in a folder (OBJ, STL, glTF, GLB), by name, sorted.
pub fn model_files(dir: &std::path::Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
                    ["obj", "stl", "gltf", "glb"].contains(&ext.as_str()).then_some(name)
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// Read a scene file. Its models are then looked for beside it.
pub fn load_scene(path: &std::path::Path) -> Result<Scene, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("couldn't read {}: {e}", path.display()))?;
    let mut scene = parse_scene(&text)?;
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(std::path::Path::new("."));
    scene.base = Some(std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf()));
    Ok(scene)
}

/// Read a scene from JSON — tolerating a fenced block around it.
pub fn parse_scene(text: &str) -> Result<Scene, String> {
    let t = text.trim();
    let body = match (t.find('{'), t.rfind('}')) {
        (Some(a), Some(b)) if b > a => &t[a..=b],
        _ => return Err("there's no scene in that — no JSON object".into()),
    };
    serde_json::from_str(body).map_err(|e| format!("the scene doesn't read: {e}"))
}

fn name_of(i: usize, o: &Object) -> String {
    let kind = match o.shape {
        Shape::Sphere { .. } => "sphere",
        Shape::Box { .. } => "box",
        Shape::Cylinder { .. } => "cylinder",
        Shape::Cone { .. } => "cone",
        Shape::Capsule { .. } => "capsule",
        Shape::Torus { .. } => "torus",
        Shape::Mesh { .. } => "model",
        Shape::Ground { .. } => "ground",
    };
    match &o.name {
        Some(n) => format!("{n} ({kind})"),
        None => format!("object {} ({kind})", i + 1),
    }
}

/// Roughly where an object's middle is (for "is it in view").
fn middle(o: &Object) -> V {
    let up = match o.shape {
        Shape::Cylinder { height, .. } | Shape::Cone { height, .. } | Shape::Capsule { height, .. } => height / 2.0,
        Shape::Mesh { fit: Some(f), .. } => f / 2.0,
        _ => 0.0,
    };
    add(o.at, M3::euler_deg(o.rotate).apply([0.0, up * o.scale, 0.0]))
}

/// What can be said about a scene before drawing it.
pub fn check_scene(scene: &Scene) -> Vec<Finding> {
    let mut out = Vec::new();
    let block = |rule: &str, detail: String| Finding { severity: Severity::Blocking, rule: rule.into(), detail };
    let advise = |rule: &str, detail: String| Finding { severity: Severity::Advisory, rule: rule.into(), detail };
    let solid: Vec<(usize, &Object)> = scene.objects.iter().enumerate().filter(|(_, o)| !matches!(o.shape, Shape::Ground { .. })).collect();
    if solid.is_empty() {
        out.push(block("has something in it", "the scene has nothing to look at — no sphere, box, cylinder, cone, capsule, torus or model".into()));
    }
    for (i, o) in scene.objects.iter().enumerate() {
        if let Shape::Mesh { file, fit } = &o.shape {
            match fitted(file, *fit, scene.base.as_deref()) {
                Err(e) => out.push(block("models read", format!("{}: {e}", name_of(i, o)))),
                Ok(m) if fit.is_none() && (m.mesh.hi[1] - m.mesh.lo[1]).max(m.mesh.hi[0] - m.mesh.lo[0]) > 200.0 => out.push(advise(
                    "models to scale",
                    format!("{} is {:.0} units across — if those are millimetres, give it a \"fit\" in metres", name_of(i, o), (m.mesh.hi[0] - m.mesh.lo[0]).max(m.mesh.hi[1] - m.mesh.lo[1]))
                )),
                Ok(_) => {}
            }
        }
        let kind = match &o.pattern {
            Some(Pattern::Kind(k)) | Some(Pattern::Full { kind: k, .. }) => Some(k),
            None => None,
        };
        if let Some(k) = kind.filter(|k| pattern_kind(k).is_none()) {
            out.push(advise("patterns known", format!("{}: \"{k}\" isn't a pattern (checker, stripes, grid, dots, noise), so it's drawn plain", name_of(i, o))));
        }
        if o.material == Material::Glass && matches!(o.shape, Shape::Ground { .. }) {
            out.push(advise("glass has a back", "glass ground has no underside to leave by, so it's drawn as a floor of glass over sky".into()));
        }
    }
    if scene.width < 16 || scene.height < 16 || scene.width > 4096 || scene.height > 4096 {
        out.push(block("a sensible size", format!("{}×{} isn't a size to draw at (16 to 4096 a side)", scene.width, scene.height)));
    }
    if !(0.0..=60.0).contains(&scene.duration) || !scene.duration.is_finite() {
        out.push(block("a sensible length", format!("{} s isn't a length to animate (0 to 60)", scene.duration)));
    }
    if scene.duration > 0.0 && !(1..=60).contains(&scene.fps) {
        out.push(block("a sensible frame rate", format!("{} frames a second isn't one to animate at (1 to 60)", scene.fps)));
    }
    let tracks = scene.objects.iter().flat_map(|o| o.animate.iter()).chain(scene.camera.animate.iter());
    for tr in tracks {
        let good = tr.keys.iter().filter(|(_, v)| value3(v).is_some()).count();
        if good == 0 {
            out.push(block("keyframes read", format!("the \"{}\" track has no keys that read as a number, [x, y, z] or a colour", tr.prop)));
            continue;
        }
        if tr.keys.windows(2).any(|w| w[1].0 < w[0].0) {
            out.push(block("keyframes in order", format!("the \"{}\" track's keys aren't in time order", tr.prop)));
        }
        if scene.duration > 0.0 && tr.keys.iter().any(|(t, _)| *t > scene.duration + 1e-9 || *t < 0.0) {
            out.push(advise("keyframes in the timeline", format!("the \"{}\" track has keys outside 0–{} s; those moments are never shown", tr.prop, scene.duration)));
        }
        let known = ["at", "position", "rotate", "rotation", "scale", "size", "colour", "color", "from", "target", "fov"];
        if !known.contains(&tr.prop.as_str()) {
            out.push(advise("keyframes do something", format!("\"{}\" isn't something that can be animated, so that track does nothing", tr.prop)));
        }
    }
    let moves = scene.camera.orbit != 0.0
        || !scene.camera.animate.is_empty()
        || scene.objects.iter().any(|o| !o.animate.is_empty() || o.spin != [0.0; 3]);
    if scene.duration > 0.0 && !moves {
        out.push(block("something moves", format!("it's {} s long but nothing in it is animated — no keyframes, spin or orbit", scene.duration)));
    }
    if moves && scene.duration <= 0.0 {
        out.push(advise("a length to move in", "things are set to move but the duration is 0, so it's drawn as a still".into()));
    }
    let c = &scene.camera;
    if len(sub(c.at, c.from)) < 1e-9 {
        out.push(block("camera looks somewhere", "the camera stands on the point it's looking at, so it has no direction".into()));
        return out;
    }
    // In view, all the way through (sampled at up to 24 moments).
    let times = frame_times(scene);
    let step = (times.len() / 24).max(1);
    for (i, o) in &solid {
        let mut gone: Vec<f64> = Vec::new();
        for &t in times.iter().step_by(step) {
            let now = object_at(o, t);
            let inside = project(scene, t, middle(&now))
                .map(|(x, y)| x >= 0.0 && y >= 0.0 && x <= scene.width as f64 && y <= scene.height as f64)
                .unwrap_or(false);
            if !inside {
                gone.push(t);
            }
        }
        if !gone.is_empty() {
            let when = if scene.duration <= 0.0 {
                String::new()
            } else if gone.len() * step >= times.len() {
                " the whole time".into()
            } else {
                format!(" from about {:.1} s to {:.1} s", gone[0], gone[gone.len() - 1])
            };
            out.push(advise("in view", format!("{} is out of the picture{when}", name_of(*i, o))));
        }
    }
    out
}

/// After drawing: is anything but sky in the picture? Blocking when fewer
/// than one pixel in two hundred shows a surface.
fn picture_findings(scene: &Scene, img: &Rgba) -> Vec<Finding> {
    let light = light_of(scene);
    let (fwd, right, up, half_h) = camera_frame(&scene_at(scene, 0.0).camera);
    let aspect = img.width as f64 / img.height as f64;
    let mut surface = 0usize;
    let n = (img.width * img.height) as usize;
    for y in 0..img.height {
        for x in 0..img.width {
            let u = ((x as f64 + 0.5) / img.width as f64 * 2.0 - 1.0) * half_h * aspect;
            let v = (1.0 - (y as f64 + 0.5) / img.height as f64 * 2.0) * half_h;
            let d = unit(add(fwd, add(mul(right, u), mul(up, v))));
            let s = sky(&light, d);
            let want = [screen_byte(s[0]) as i32, screen_byte(s[1]) as i32, screen_byte(s[2]) as i32];
            let got = img.at(x, y);
            if (0..3).any(|c| (got[c] as i32 - want[c]).abs() > 10) {
                surface += 1;
            }
        }
    }
    if surface * 200 < n {
        return vec![Finding {
            severity: Severity::Blocking,
            rule: "something was drawn".into(),
            detail: "the picture came out as sky — the camera isn't pointed at anything".into(),
        }];
    }
    Vec::new()
}

// ------------------------------------------------------------------ Blender

/// The scene as a Blender script: the same shapes, colours, sun, sky and
/// camera, rendered with Cycles on the CPU to `out` at the scene's size.
/// Motion is baked: every frame's position, turn, size and colour, worked out
/// by this module's own timeline, is keyed — so Blender moves exactly as the
/// in-house render does. A still scene renders one picture to `out`; a moving
/// one renders frames `out` + "0001.png" … Blender is z-up, so (x, y, z) here
/// is (x, −z, y) there.
pub fn blender_script(scene: &Scene, out: &std::path::Path) -> String {
    let bv = |v: V| format!("({:.5}, {:.5}, {:.5})", v[0], -v[2], v[1]);
    let rgba = |c: V| format!("({:.5}, {:.5}, {:.5}, 1.0)", c[0], c[1], c[2]);
    // Our basis to Blender's: x→x, y→z, z→−y.
    let conv = |m: M3| -> [[f64; 3]; 3] {
        let c = [[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]];
        let mul3 = |a: [[f64; 3]; 3], b: [[f64; 3]; 3]| {
            let mut r = [[0.0; 3]; 3];
            for i in 0..3 {
                for j in 0..3 {
                    r[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
                }
            }
            r
        };
        let ct = [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]];
        mul3(mul3(c, m.0), ct)
    };
    let mat = |m: [[f64; 3]; 3]| {
        format!(
            "(({:.6},{:.6},{:.6}),({:.6},{:.6},{:.6}),({:.6},{:.6},{:.6}))",
            m[0][0], m[0][1], m[0][2], m[1][0], m[1][1], m[1][2], m[2][0], m[2][1], m[2][2]
        )
    };
    let times = frame_times(scene);
    let moving = scene.duration > 0.0;
    let mut s = String::new();
    s.push_str("import bpy, math, mathutils\n");
    s.push_str("bpy.ops.wm.read_factory_settings(use_empty=True)\n");
    s.push_str("scene = bpy.context.scene\n");
    s.push_str(BLENDER_HELPERS);
    s.push_str("def pose(obj, frames):\n");
    s.push_str("    prev = None\n");
    s.push_str("    for f, loc, rot, sc in frames:\n");
    s.push_str("        obj.location = loc\n");
    s.push_str("        e = mathutils.Matrix(rot).to_euler('XYZ', prev) if prev else mathutils.Matrix(rot).to_euler('XYZ')\n");
    s.push_str("        obj.rotation_euler = e\n        prev = e\n");
    s.push_str("        obj.scale = (sc, sc, sc)\n");
    s.push_str("        if frames_moving:\n");
    s.push_str("            for path in ('location', 'rotation_euler', 'scale'):\n");
    s.push_str("                obj.keyframe_insert(data_path=path, frame=f)\n");
    s.push_str(&format!("frames_moving = {}\n", if moving { "True" } else { "False" }));
    for (i, o) in scene.objects.iter().enumerate() {
        let geometry = match &o.shape {
            Shape::Sphere { radius } => format!("bpy.ops.mesh.primitive_uv_sphere_add(radius={radius:.5}, segments=64, ring_count=32)\nbpy.ops.object.shade_smooth()\n"),
            Shape::Box { size } => format!(
                "bpy.ops.mesh.primitive_cube_add(size=1)\nbpy.context.object.data.transform(mathutils.Matrix.Diagonal(({:.5}, {:.5}, {:.5}, 1.0)))\n",
                size[0], size[2], size[1]
            ),
            Shape::Cylinder { radius, height } => format!(
                "bpy.ops.mesh.primitive_cylinder_add(radius={radius:.5}, depth={height:.5}, vertices=64)\nbpy.context.object.data.transform(mathutils.Matrix.Translation((0, 0, {:.5})))\n",
                height / 2.0
            ),
            Shape::Cone { radius, height } => format!(
                "bpy.ops.mesh.primitive_cone_add(radius1={radius:.5}, radius2=0, depth={height:.5}, vertices=64)\nbpy.context.object.data.transform(mathutils.Matrix.Translation((0, 0, {:.5})))\n",
                height / 2.0
            ),
            Shape::Capsule { radius, height } => format!(
                "bpy.ops.mesh.primitive_cylinder_add(radius={radius:.5}, depth={body:.5}, vertices=64)\nbody_{i} = bpy.context.object\nbody_{i}.data.transform(mathutils.Matrix.Translation((0, 0, {mid:.5})))\n\
                 for cz in ({lo:.5}, {hi:.5}):\n    bpy.ops.mesh.primitive_uv_sphere_add(radius={radius:.5}, segments=48, ring_count=24, location=(0, 0, cz))\n    body_{i}.select_set(True)\n    bpy.context.view_layer.objects.active = body_{i}\n    bpy.ops.object.join()\n\
                 bpy.ops.object.shade_smooth()\n",
                body = (height - 2.0 * radius).max(1e-4),
                mid = height / 2.0,
                lo = radius,
                hi = (height - radius).max(*radius)
            ),
            Shape::Torus { radius, thickness } => format!(
                "bpy.ops.mesh.primitive_torus_add(major_radius={radius:.5}, minor_radius={thickness:.5}, major_segments=96, minor_segments=32)\nbpy.ops.object.shade_smooth()\n"
            ),
            Shape::Ground { y, .. } => format!("bpy.ops.mesh.primitive_plane_add(size=400, location=(0, 0, {y:.5}))\n"),
            Shape::Mesh { file, fit } => match fitted(file, *fit, scene.base.as_deref()) {
                Ok(m) => {
                    let path = model_path(file, scene.base.as_deref());
                    let path = std::path::absolute(&path).unwrap_or(path);
                    let kind = match path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref() {
                        Some("obj") => "obj",
                        Some("stl") => "stl",
                        _ => "gltf",
                    };
                    format!("brought = bring({:?}, '{kind}', {}, {:.8})\n", path.display().to_string(), bv(m.centre), m.k)
                }
                // check_scene has said why; Blender leaves it out too.
                Err(_) => "bpy.ops.object.empty_add()\n".into(),
            },
        };
        s.push_str(&format!("# {}\n", name_of(i, o)));
        s.push_str(&geometry);
        // A model is the object `bring` hands back (the importer may leave
        // something else current); anything else is what was just added.
        if matches!(o.shape, Shape::Mesh { .. }) && geometry.starts_with("brought") {
            s.push_str(&format!("obj_{i} = brought\n"));
        } else {
            s.push_str(&format!("obj_{i} = bpy.context.object\n"));
        }
        s.push_str(&format!("bsdf_{i} = {}\n", blender_paint(i, o, scene.base.as_deref())));
        if matches!(o.shape, Shape::Ground { .. }) {
            continue;
        }
        let frames: Vec<String> = times
            .iter()
            .enumerate()
            .map(|(f, &t)| {
                let now = object_at(o, t);
                format!("({}, {}, {}, {:.5})", f + 1, bv(now.at), mat(conv(M3::euler_deg(now.rotate))), now.scale)
            })
            .collect();
        s.push_str(&format!("pose(obj_{i}, [{}])\n", frames.join(", ")));
        if moving && o.animate.iter().any(|t| t.prop == "colour" || t.prop == "color") {
            for (f, &t) in times.iter().enumerate() {
                let c = linear(&object_at(o, t).colour);
                s.push_str(&format!(
                    "if bsdf_{i}:\n    bsdf_{i}.inputs['Base Color'].default_value = {}\n    bsdf_{i}.inputs['Base Color'].keyframe_insert('default_value', frame={})\n",
                    rgba(c),
                    f + 1
                ));
            }
        }
    }
    let (sun_from, softness, sun_col, strength) = match &scene.sun {
        Sun::From(v) => (*v, default_softness(), default_sun_colour(), 1.0),
        Sun::Full { from, softness, colour, strength } => (*from, *softness, colour.clone(), *strength),
    };
    s.push_str(&format!(
        "bpy.ops.object.light_add(type='SUN', location=(0, 0, 10))\nsun = bpy.context.object\nsun.data.energy = {:.3}\nsun.data.angle = math.radians({:.3})\nsun.data.color = {}\nsun.rotation_euler = mathutils.Vector({}).to_track_quat('Z', 'Y').to_euler()\n",
        3.0 * strength,
        softness,
        {
            let c = linear(&sun_col);
            format!("({:.4}, {:.4}, {:.4})", c[0], c[1], c[2])
        },
        bv(unit(sun_from))
    ));
    s.push_str("bpy.ops.object.camera_add()\ncam = bpy.context.object\ncam.data.sensor_fit = 'VERTICAL'\nscene.camera = cam\n");
    s.push_str("prev = None\n");
    for (f, &t) in times.iter().enumerate() {
        let c = camera_at(&scene.camera, scene.duration, t);
        s.push_str(&format!(
            "cam.location = {}\ncam.data.angle_y = math.radians({:.4})\nq = (mathutils.Vector({}) - cam.location).to_track_quat('-Z', 'Y')\ne = q.to_euler('XYZ', prev) if prev else q.to_euler('XYZ')\ncam.rotation_euler = e\nprev = e\n",
            bv(c.from),
            c.fov,
            bv(c.at)
        ));
        if moving {
            s.push_str(&format!(
                "for path in ('location', 'rotation_euler'):\n    cam.keyframe_insert(data_path=path, frame={0})\ncam.data.keyframe_insert(data_path='lens', frame={0})\n",
                f + 1
            ));
        }
    }
    let sky = linear(&scene.sky);
    s.push_str(&format!(
        "world = bpy.data.worlds.new('w')\nworld.use_nodes = True\nworld.node_tree.nodes['Background'].inputs['Color'].default_value = ({:.4}, {:.4}, {:.4}, 1.0)\nscene.world = world\n",
        sky[0], sky[1], sky[2]
    ));
    s.push_str("scene.render.engine = 'CYCLES'\nscene.cycles.device = 'CPU'\nscene.cycles.samples = 32\n");
    s.push_str("scene.view_settings.view_transform = 'Filmic' if 'Filmic' in [i.identifier for i in scene.view_settings.bl_rna.properties['view_transform'].enum_items] else scene.view_settings.view_transform\n");
    s.push_str(&format!(
        "scene.render.resolution_x = {}\nscene.render.resolution_y = {}\nscene.render.resolution_percentage = 100\n",
        scene.width, scene.height
    ));
    s.push_str("scene.render.image_settings.file_format = 'PNG'\n");
    if moving {
        s.push_str(&format!(
            "scene.render.fps = {}\nscene.frame_start = 1\nscene.frame_end = {}\nscene.render.use_motion_blur = {}\n",
            scene.fps.clamp(1, 60),
            times.len(),
            if scene.motion_blur { "True" } else { "False" }
        ));
        s.push_str(&format!("scene.render.filepath = r'{}'\n", out.display()));
        s.push_str("bpy.ops.render.render(animation=True)\n");
    } else {
        s.push_str(&format!("scene.render.filepath = r'{}'\n", out.display()));
        s.push_str("bpy.ops.render.render(write_still=True)\n");
    }
    // Out at once: the pictures are written, and Blender 4.2 as a Python
    // module was seen to crash tidying up after the glTF importer — which
    // made a good render look like a failed one.
    s.push_str("import os\nos._exit(0)\n");
    s
}

/// The Blender side of an object's look: colour, shine, glass, glow and
/// pattern, as Principled BSDF inputs and shader nodes.
fn blender_paint(i: usize, o: &Object, base: Option<&std::path::Path>) -> String {
    let p = place(o, base);
    let rgba = |c: V| format!("({:.5}, {:.5}, {:.5}, 1.0)", c[0], c[1], c[2]);
    // A model's own colours stay when nothing asks for another look.
    let keep = p.model.as_ref().map(|m| !m.mesh.colours.is_empty()).unwrap_or(false) && o.material == Material::Solid && p.pattern.is_none();
    let pat = match &p.pattern {
        Some(pt) => format!("('{:?}', {}, {:.6})", pt.kind, rgba(pt.other), pt.size).to_lowercase(),
        None => "None".into(),
    };
    format!(
        "paint(obj_{i}, {}, {:.3}, ior={}, glow={}, pat={pat}, keep={})",
        rgba(p.albedo),
        p.shine,
        p.glass.map(|g| format!("{g:.4}")).unwrap_or_else(|| "None".into()),
        if p.glow > 0.0 { format!("{:.4}", p.glow) } else { "None".into() },
        if keep { "True" } else { "False" }
    )
}

/// Python for the Blender script: painting an object (with glass, glow and
/// the patterns built from shader nodes so they match the in-house ones —
/// checker, stripes, grid and dots exactly; noise only in kind), and bringing
/// in a model file, joined into one object and moved and sized the way
/// `scene3d` fits it.
const BLENDER_HELPERS: &str = r#"def paint(obj, rgba, shine, ior=None, glow=None, pat=None, keep=False):
    if obj.type != 'MESH':
        return None
    if keep and len(obj.data.materials) > 0:
        return None
    obj.data.materials.clear()
    m = bpy.data.materials.new('m')
    m.use_nodes = True
    nt = m.node_tree
    p = nt.nodes.get('Principled BSDF')
    p.inputs['Base Color'].default_value = rgba
    p.inputs['Roughness'].default_value = 1.0 - 0.9 * shine
    if ior is not None:
        p.inputs['Transmission Weight' if 'Transmission Weight' in p.inputs else 'Transmission'].default_value = 1.0
        p.inputs['IOR'].default_value = ior
        p.inputs['Roughness'].default_value = 0.0
    if glow is not None:
        p.inputs['Emission Color' if 'Emission Color' in p.inputs else 'Emission'].default_value = rgba
        p.inputs['Emission Strength'].default_value = glow
    if pat is not None:
        kind, other, size = pat
        tc = nt.nodes.new('ShaderNodeTexCoord')
        mp = nt.nodes.new('ShaderNodeMapping')
        # Blender's object space is z-up: (x, y, z) here is (x, -z, y) there.
        mp.inputs['Scale'].default_value = (1.0 / size, -1.0 / size, 1.0 / size)
        nt.links.new(tc.outputs['Object'], mp.inputs['Vector'])
        v = mp.outputs['Vector']
        def op(kind_, a, b=None, vec=False):
            n = nt.nodes.new('ShaderNodeVectorMath' if vec else 'ShaderNodeMath')
            n.operation = kind_
            for k, x in enumerate((a, b)):
                if x is None:
                    continue
                if isinstance(x, (int, float)):
                    n.inputs[k].default_value = (x, x, x) if vec else x
                else:
                    nt.links.new(x, n.inputs[k])
            return n.outputs['Value'] if (not vec or kind_ in ('DOT_PRODUCT', 'LENGTH')) else n.outputs['Vector']
        sep = nt.nodes.new('ShaderNodeSeparateXYZ')
        nt.links.new(v, sep.inputs['Vector'])
        X, Y, Z = sep.outputs['X'], sep.outputs['Z'], sep.outputs['Y']
        if kind == 'checker':
            fac = op('FLOORED_MODULO', op('ADD', op('ADD', op('FLOOR', X), op('FLOOR', Y)), op('FLOOR', Z)), 2.0)
        elif kind == 'stripes':
            fac = op('FLOORED_MODULO', op('FLOOR', Y), 2.0)
        elif kind == 'grid':
            def edge(a):
                f = op('SUBTRACT', a, op('FLOOR', a))
                return op('MINIMUM', f, op('SUBTRACT', 1.0, f))
            fac = op('LESS_THAN', op('MINIMUM', op('MINIMUM', edge(X), edge(Y)), edge(Z)), 0.06)
        elif kind == 'dots':
            c = op('SUBTRACT', op('FRACTION', v, vec=True), 0.5, vec=True)
            fac = op('LESS_THAN', op('DOT_PRODUCT', c, c, vec=True), 0.09)
        else:
            nz = nt.nodes.new('ShaderNodeTexNoise')
            nz.inputs['Scale'].default_value = 1.0
            nz.inputs['Detail'].default_value = 2.0
            nt.links.new(v, nz.inputs['Vector'])
            fac = op('ADD', op('MULTIPLY', op('SUBTRACT', nz.outputs['Fac'], 0.5), 1.6), 0.5)
            fac = op('MINIMUM', op('MAXIMUM', fac, 0.0), 1.0)
        mx = nt.nodes.new('ShaderNodeMix')
        mx.data_type = 'RGBA'
        ins = [s for s in mx.inputs if s.enabled]
        nt.links.new(fac, ins[0])
        ins[1].default_value = rgba
        ins[2].default_value = other
        out = [s for s in mx.outputs if s.enabled][0]
        nt.links.new(out, p.inputs['Base Color'])
    obj.data.materials.append(m)
    return p
def bring(path, kind, centre, k):
    try:
        import addon_utils
        getattr(addon_utils, 'enable')('io_scene_gltf2', default_set=True)
    except Exception:
        pass
    before = set(bpy.data.objects)
    if kind == 'obj':
        bpy.ops.wm.obj_import(filepath=path, forward_axis='NEGATIVE_Z', up_axis='Y')
    elif kind == 'stl':
        bpy.ops.wm.stl_import(filepath=path, forward_axis='NEGATIVE_Z', up_axis='Y')
    else:
        bpy.ops.import_scene.gltf(filepath=path)
    new = [o for o in bpy.data.objects if o not in before]
    meshes = [o for o in new if o.type == 'MESH']
    for o in meshes:
        mw = o.matrix_world.copy()
        o.data = o.data.copy()
        o.parent = None
        o.data.transform(mw)
        o.matrix_world = mathutils.Matrix.Identity(4)
    for o in new:
        if o.type != 'MESH':
            bpy.data.objects.remove(o)
    bpy.ops.object.select_all(action='DESELECT')
    for o in meshes:
        o.select_set(True)
    bpy.context.view_layer.objects.active = meshes[0]
    if len(meshes) > 1:
        bpy.ops.object.join()
    obj = bpy.context.view_layer.objects.active
    # The glTF importer turns objects by quaternion; `pose` sets Euler angles.
    obj.rotation_mode = 'XYZ'
    obj.data.transform(mathutils.Matrix.Scale(k, 4) @ mathutils.Matrix.Translation(-mathutils.Vector(centre)))
    return obj
"#;

/// Blender, if it's installed: the tools setting (`vars.blender`), else where
/// Blender's Windows installer puts it (newest version first), else the PATH.
pub fn find_blender(configured: Option<&str>) -> Option<std::path::PathBuf> {
    if let Some(p) = configured.filter(|c| !c.trim().is_empty()).and_then(crate::tools::which) {
        return Some(p.into());
    }
    if cfg!(windows) {
        if let Ok(pf) = std::env::var("ProgramFiles") {
            let root = std::path::Path::new(&pf).join("Blender Foundation");
            let mut found: Vec<std::path::PathBuf> = std::fs::read_dir(&root)
                .map(|rd| rd.flatten().map(|e| e.path().join("blender.exe")).filter(|p| p.is_file()).collect())
                .unwrap_or_default();
            found.sort();
            if let Some(p) = found.pop() {
                return Some(p);
            }
        }
    }
    crate::tools::which("blender").map(Into::into)
}

/// Render with Blender and check what came out. `Err` when Blender couldn't
/// be run at all. A moving scene's frames land as `<stem>.blender.0001.png` …
fn render_in_blender(scene: &Scene, blender: &std::path::Path, dir: &std::path::Path, stem: &str) -> Result<(std::path::PathBuf, Vec<Finding>), String> {
    let moving = scene.duration > 0.0;
    let out = if moving { dir.join(format!("{stem}.blender.")) } else { dir.join(format!("{stem}.blender.png")) };
    let script = dir.join(format!(".{stem}.blender.py"));
    std::fs::write(&script, blender_script(scene, &out)).map_err(|e| format!("couldn't write the Blender script: {e}"))?;
    let status = crate::tools::command(blender)
        .arg("-b")
        .arg("--factory-startup")
        .arg("--python")
        .arg(&script)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("couldn't run Blender: {e}"))?;
    if !status.success() {
        return Err(format!("Blender stopped with {status}"));
    }
    let expect = crate::motion::Expect { kind: crate::motion::RenderKind::Png, width: scene.width, height: scene.height };
    if !moving {
        return Ok((out.clone(), crate::motion::verify_render(&out, &expect)));
    }
    let n = frame_times(scene).len();
    let mut findings = Vec::new();
    let mut missing = 0;
    for f in 1..=n {
        let p = dir.join(format!("{stem}.blender.{f:04}.png"));
        if p.is_file() {
            if f == 1 {
                findings.extend(crate::motion::verify_render(&p, &expect));
            }
        } else {
            missing += 1;
        }
    }
    if missing > 0 {
        findings.push(Finding {
            severity: Severity::Blocking,
            rule: "every frame rendered".into(),
            detail: format!("Blender rendered {} of the {n} frames", n - missing),
        });
    }
    Ok((dir.join(format!("{stem}.blender.0001.png")), findings))
}

// ----------------------------------------------------------- making it all

/// Ask a model for a scene, and give its complaints back until it reads and
/// has something in view, up to `rounds` fixes. `Err` carries why it never got
/// there.
/// `models` is the folder its model files are found in.
pub fn draft_scene(idea: &str, llm: &dyn crate::brain::Llm, rounds: u32, models: Option<&std::path::Path>) -> Result<Scene, String> {
    let mut reply = llm.complete(SCENE_SYSTEM, idea).map_err(|e| e.to_string())?;
    let mut tries = 0;
    let moving = asks_for_motion(idea);
    loop {
        let problem = match parse_scene(&reply) {
            Ok(mut scene) => {
                scene.base = models.map(|m| m.to_path_buf());
                let mut blocking: Vec<String> =
                    check_scene(&scene).into_iter().filter(|f| f.severity == Severity::Blocking).map(|f| f.detail).collect();
                if moving && scene.duration <= 0.0 {
                    blocking.push("you were asked for something that moves, but the duration is 0 — give it a duration and animate it".into());
                }
                if blocking.is_empty() {
                    return Ok(scene);
                }
                blocking.join("; ")
            }
            Err(e) => e,
        };
        if tries >= rounds {
            return Err(problem);
        }
        tries += 1;
        let ask = format!("{idea}\n\nYour last scene had a problem: {problem}. Return the whole corrected JSON.\n\n{reply}");
        reply = llm.complete(SCENE_SYSTEM, &ask).map_err(|e| e.to_string())?;
    }
}

/// Does the request ask for movement? ("animate", "spinning", "bouncing", a
/// length in seconds…) — then a still scene is not an answer.
fn asks_for_motion(idea: &str) -> bool {
    let low = idea.to_lowercase();
    let words = [
        "animat", "moving", "moves", "move ", "spin", "rotat", "turning", "bounc", "roll", "orbit", "fly", "flying",
        "falling", "drop", "swing", "slide", "sliding", "grow", "shrink", "pulse", "wobbl", "dance", "second",
    ];
    words.iter().any(|w| low.contains(w))
}

/// What drawing a scene made.
#[derive(Debug, Clone)]
pub struct SceneMade {
    pub still: std::path::PathBuf,
    /// The camera circling a still scene.
    pub turntable: Option<std::path::PathBuf>,
    /// A moving scene, as a GIF.
    pub gif: Option<std::path::PathBuf>,
    /// A moving scene, as an MP4 (needs ffmpeg).
    pub mp4: Option<std::path::PathBuf>,
    pub frames: usize,
    /// How many of them differ from the one before: 0 means nothing moved.
    pub changed: usize,
    pub blender: Option<std::path::PathBuf>,
    pub findings: Vec<Finding>,
    pub notes: Vec<String>,
    pub seconds_to_draw: f64,
}

impl SceneMade {
    pub fn say(&self) -> String {
        let mut s = match (&self.gif, &self.mp4) {
            (Some(g), Some(m)) => format!("Animated it: {} frames, as a GIF ({}) and an MP4 ({}).", self.frames, g.display(), m.display()),
            (Some(g), None) => format!("Animated it: {} frames, as a GIF ({}).", self.frames, g.display()),
            _ => format!("Drew it: {}.", self.still.display()),
        };
        if let Some(t) = &self.turntable {
            s.push_str(&format!(" Walked the camera round it: {}.", t.display()));
        }
        if self.gif.is_some() {
            s.push_str(&format!(" The first frame is {}.", self.still.display()));
        }
        if let Some(b) = &self.blender {
            s.push_str(&format!(" Blender's render: {}.", b.display()));
        }
        s.push_str(&format!(" ({:.1} s to draw.)", self.seconds_to_draw));
        for f in &self.findings {
            s.push_str(&format!("\n  • {}", f.detail));
        }
        for n in &self.notes {
            s.push_str(&format!(" ({n}.)"));
        }
        s.push_str(" That it drew, stays in view and moves as keyed is checked; whether it looks right is yours to say.");
        s
    }
}

/// Draw `scene` into `dir`. A still scene: `<stem>.png` and a turntable GIF of
/// `turn_frames` (0 for none). A moving one: every frame, `<stem>.gif`,
/// `<stem>.mp4` when ffmpeg is there, and the first frame as `<stem>.png`.
/// Blender renders it too when `blender` is given.
pub fn make(scene: &Scene, dir: &std::path::Path, stem: &str, turn_frames: usize, blender: Option<&std::path::Path>) -> Result<SceneMade, String> {
    let dir = &std::path::absolute(dir).map_err(|e| format!("couldn't place {}: {e}", dir.display()))?;
    std::fs::create_dir_all(dir).map_err(|e| format!("couldn't make {}: {e}", dir.display()))?;
    let started = std::time::Instant::now();
    let mut findings = check_scene(scene);
    let frames = render_animation(scene);
    let first = frames.first().ok_or("no frames were drawn")?;
    findings.extend(picture_findings(scene, first));
    let still = dir.join(format!("{stem}.png"));
    std::fs::write(&still, crate::pngcodec::write_png(first)).map_err(|e| format!("couldn't save the picture: {e}"))?;
    let changed = frames.windows(2).filter(|w| w[0].pixels != w[1].pixels).count();
    let mut made = SceneMade {
        still,
        turntable: None,
        gif: None,
        mp4: None,
        frames: frames.len(),
        changed,
        blender: None,
        findings,
        notes: Vec::new(),
        seconds_to_draw: 0.0,
    };
    if scene.duration > 0.0 {
        if changed == 0 && frames.len() > 1 {
            made.findings.push(Finding {
                severity: Severity::Blocking,
                rule: "moves when played".into(),
                detail: format!("all {} frames came out the same picture — nothing moved", frames.len()),
            });
        }
        let delay = ((100.0 / scene.fps.clamp(1, 60) as f64).round() as u16).max(2);
        let gif_frames: Vec<crate::gifenc::Frame> = frames.iter().map(|f| crate::gifenc::Frame { image: f, delay_cs: delay }).collect();
        let gif = crate::gifenc::encode_gif_dithered(&gif_frames)?;
        let path = dir.join(format!("{stem}.gif"));
        std::fs::write(&path, gif).map_err(|e| format!("couldn't save the GIF: {e}"))?;
        made.gif = Some(path);
        let work = dir.join(format!(".{stem}.frames"));
        let _ = std::fs::remove_dir_all(&work);
        std::fs::create_dir_all(&work).map_err(|e| format!("couldn't make a frames folder: {e}"))?;
        for (i, f) in frames.iter().enumerate() {
            std::fs::write(work.join(format!("frame{i:04}.png")), crate::pngcodec::write_png(f)).map_err(|e| format!("couldn't save a frame: {e}"))?;
        }
        match crate::tools::which("ffmpeg") {
            Some(ff) => {
                let mp4 = dir.join(format!("{stem}.mp4"));
                let _ = std::fs::remove_file(&mp4);
                let ok = crate::tools::command(ff)
                    .args(["-hide_banner", "-loglevel", "error", "-y", "-framerate"])
                    .arg(scene.fps.clamp(1, 60).to_string())
                    .arg("-i")
                    .arg(work.join("frame%04d.png"))
                    .args(["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2", "-pix_fmt", "yuv420p", "-crf", "18", "-movflags", "+faststart"])
                    .arg(&mp4)
                    .stdin(std::process::Stdio::null())
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);
                if ok {
                    let expect = crate::motion::Expect { kind: crate::motion::RenderKind::Mp4, width: scene.width, height: scene.height };
                    made.findings.extend(crate::motion::verify_render(&mp4, &expect));
                    made.mp4 = Some(mp4);
                } else {
                    made.notes.push("no MP4: ffmpeg didn't finish it".into());
                }
            }
            None => made.notes.push("no MP4: that needs ffmpeg, and it isn't installed".into()),
        }
        let _ = std::fs::remove_dir_all(&work);
    } else if turn_frames > 1 {
        let frames = turntable(scene, turn_frames);
        let gif_frames: Vec<crate::gifenc::Frame> = frames.iter().map(|f| crate::gifenc::Frame { image: f, delay_cs: 8 }).collect();
        let gif = crate::gifenc::encode_gif_dithered(&gif_frames)?;
        let path = dir.join(format!("{stem}.turntable.gif"));
        std::fs::write(&path, gif).map_err(|e| format!("couldn't save the turntable: {e}"))?;
        made.turntable = Some(path);
    }
    match blender {
        Some(b) => match render_in_blender(scene, b, dir, stem) {
            Ok((path, f)) => {
                made.findings.extend(f);
                made.blender = Some(path);
            }
            Err(e) => made.notes.push(format!("Blender didn't render: {e}")),
        },
        None => made.notes.push("no Blender render: Blender isn't installed, and the in-house one stands on its own".into()),
    }
    made.seconds_to_draw = started.elapsed().as_secs_f64();
    Ok(made)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(shape: Shape, at: V) -> Object {
        Object { shape, at, rotate: [0.0; 3], scale: 1.0, colour: grey(), shine: 0.0, name: None, spin: [0.0; 3], animate: vec![], material: Material::Solid, ior: None, glow: None, pattern: None }
    }

    #[test]
    fn every_easing_starts_at_zero_and_ends_at_one() {
        for e in [Ease::Linear, Ease::EaseIn, Ease::EaseOut, Ease::EaseInOut, Ease::Bounce, Ease::Back, Ease::Elastic] {
            assert!(eased(e, 0.0).abs() < 1e-9, "{e:?} at 0");
            assert!((eased(e, 1.0) - 1.0).abs() < 1e-9, "{e:?} at 1");
        }
        assert_eq!(eased(Ease::Step, 0.99), 0.0);
        assert!((eased(Ease::EaseInOut, 0.5) - 0.5).abs() < 1e-9);
        // Back overshoots on the way; bounce comes back down after its first landing.
        assert!((0..100).map(|i| eased(Ease::Back, i as f64 / 100.0)).fold(0.0, f64::max) > 1.0);
        assert!(eased(Ease::Bounce, 0.5) < eased(Ease::Bounce, 1.0 / 2.75));
    }

    #[test]
    fn a_key_track_lands_on_its_keys_and_loops_when_asked() {
        let tr: Track = serde_json::from_str(r#"{"prop":"at","keys":[[0,[0,0,0]],[2,[4,0,0]]],"ease":"linear"}"#).unwrap();
        assert_eq!(sample(&tr, 0.0), Some([0.0, 0.0, 0.0]));
        assert_eq!(sample(&tr, 1.0), Some([2.0, 0.0, 0.0]));
        assert_eq!(sample(&tr, 3.0), Some([4.0, 0.0, 0.0]));
        let mut looped = tr.clone();
        looped.repeat = true;
        assert_eq!(sample(&looped, 3.0), Some([2.0, 0.0, 0.0]));
    }

    #[test]
    fn a_turned_box_is_hit_on_its_turned_face() {
        let mut b = obj(Shape::Box { size: [2.0, 2.0, 2.0] }, [0.0; 3]);
        b.rotate = [0.0, 45.0, 0.0];
        let p = place(&b, None);
        let h = hit_placed(&p, [0.0, 0.0, 5.0], [0.0, 0.0, -1.0], f64::MAX).unwrap();
        // Turned 45°, the nearest point is the edge, √2 from the middle.
        assert!((h.t - (5.0 - 2f64.sqrt())).abs() < 1e-6, "{}", h.t);
    }

    #[test]
    fn the_torus_is_found_by_sphere_tracing_where_it_is() {
        let t = obj(Shape::Torus { radius: 1.0, thickness: 0.25 }, [0.0; 3]);
        let p = place(&t, None);
        // Straight down through the tube at x = 1.
        let h = hit_placed(&p, [1.0, 5.0, 0.0], [0.0, -1.0, 0.0], f64::MAX).unwrap();
        assert!((h.t - 4.75).abs() < 1e-3, "{}", h.t);
        // Straight down through the hole misses.
        assert!(hit_placed(&p, [0.0, 5.0, 0.0], [0.0, -1.0, 0.0], f64::MAX).is_none());
    }

    #[test]
    fn a_cone_is_wide_at_the_base_and_nothing_at_the_tip() {
        let c = obj(Shape::Cone { radius: 1.0, height: 2.0 }, [0.0; 3]);
        let p = place(&c, None);
        let low = hit_placed(&p, [0.0, 0.1, 5.0], [0.0, 0.0, -1.0], f64::MAX).unwrap();
        let high = hit_placed(&p, [0.0, 1.9, 5.0], [0.0, 0.0, -1.0], f64::MAX).unwrap();
        assert!(low.t < high.t, "the base sticks out further than near the tip");
        assert!((low.t - (5.0 - 0.95)).abs() < 1e-6);
    }

    #[test]
    fn projecting_the_point_the_camera_looks_at_lands_in_the_middle() {
        let s = parse_scene(r#"{"width":200,"height":100,"camera":{"from":[0,1,5],"at":[0,1,0]},"objects":[]}"#).unwrap();
        let (x, y) = project(&s, 0.0, [0.0, 1.0, 0.0]).unwrap();
        assert!((x - 100.0).abs() < 1e-9 && (y - 50.0).abs() < 1e-9);
    }
}
