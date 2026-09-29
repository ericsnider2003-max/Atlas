//! Round 8: real objects and real materials in the 3-D renderer — model files
//! (OBJ, STL, glTF/GLB), glass, glow, patterns — and a denoiser. Measured on
//! the pictures, and against Blender 4.2 where Blender can say what's right
//! (`cargo test --test all round8 -- --nocapture`).
//!
//! The test models are Blender's own Suzanne, exported by Blender 4.2 as OBJ
//! (with MTL), STL and GLB: `docs/live/round8/models/`.

use atlas::pngcodec::Rgba;
use atlas::scene3d::{check_scene, load_scene, make, parse_scene, render_frame, Scene};
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-r8-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn models() -> PathBuf {
    std::path::absolute("docs/live/round8/models").unwrap()
}

fn a_blender() -> Option<PathBuf> {
    atlas::scene3d::find_blender(None).or_else(|| {
        let p = PathBuf::from("/tmp/claude-0/bin/blender");
        p.exists().then_some(p)
    })
}

fn px(img: &Rgba, x: u32, y: u32) -> [i32; 3] {
    let p = img.at(x, y);
    [p[0] as i32, p[1] as i32, p[2] as i32]
}

fn bright(img: &Rgba, x: u32, y: u32) -> f64 {
    let p = px(img, x, y);
    (p[0] + p[1] + p[2]) as f64 / 3.0
}

/// Pixels that aren't the background (the top-left corner's colour).
fn mask(img: &Rgba) -> Vec<bool> {
    let bg = px(img, 0, 0);
    let mut out = Vec::with_capacity((img.width * img.height) as usize);
    for y in 0..img.height {
        for x in 0..img.width {
            let p = px(img, x, y);
            out.push((0..3).map(|c| (p[c] - bg[c]).abs()).sum::<i32>() > 36);
        }
    }
    out
}

fn iou(a: &[bool], b: &[bool]) -> f64 {
    let both = a.iter().zip(b).filter(|(x, y)| **x && **y).count();
    let either = a.iter().zip(b).filter(|(x, y)| **x || **y).count();
    both as f64 / either.max(1) as f64
}

fn scene(json: &str, base: &Path) -> Scene {
    let mut s = parse_scene(json).unwrap();
    s.base = Some(base.to_path_buf());
    s
}

#[test]
fn the_same_model_reads_the_same_from_obj_stl_and_glb() {
    let m = models();
    let mut seen = Vec::new();
    for f in ["suzanne.obj", "suzanne.stl", "suzanne.glb"] {
        let mesh = atlas::meshio::read_model(&m.join(f)).unwrap();
        seen.push((f, mesh.tris.len(), mesh.lo, mesh.hi, mesh.colours.clone()));
    }
    for (f, n, lo, hi, c) in &seen {
        println!("LIVE [models] {f}: {n} triangles, from ({:.3}, {:.3}, {:.3}) to ({:.3}, {:.3}, {:.3}), colours {c:?}", lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]);
    }
    let (_, n0, lo0, hi0, _) = seen[0];
    for (f, n, lo, hi, _) in &seen[1..] {
        assert_eq!(*n, n0, "{f}");
        for k in 0..3 {
            assert!((lo[k] - lo0[k]).abs() < 1e-4 && (hi[k] - hi0[k]).abs() < 1e-4, "{f} bounds differ on axis {k}");
        }
    }
    // Suzanne at size=2 (once smoothed) is 2.65 m ear to ear and faces +z (toward a camera on +z).
    assert!((hi0[0] - lo0[0] - 2.65).abs() < 0.02, "{}", hi0[0] - lo0[0]);
    // The OBJ's colour (Kd) and the GLB's (baseColorFactor) are the same gold.
    let (obj_c, glb_c) = (&seen[0].4, &seen[2].4);
    assert_eq!(obj_c.len(), 1);
    assert_eq!(glb_c.len(), 1);
    for k in 0..3 {
        assert!((obj_c[0][k] - glb_c[0][k]).abs() < 1e-3, "{obj_c:?} vs {glb_c:?}");
    }
}

#[test]
fn a_gltf_written_by_hand_reads_its_node_tree_indices_and_colour() {
    // One triangle, indexed with u16, in a child node (moved up 5) of a node
    // moved 10 along x and doubled: the corners should land at x 10..12,
    // y 10..12.
    let mut bin: Vec<u8> = Vec::new();
    for v in [0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        bin.extend_from_slice(&v.to_le_bytes());
    }
    for i in [0u16, 1, 2, 0] {
        bin.extend_from_slice(&i.to_le_bytes()); // the last one is padding
    }
    let uri = format!("data:application/octet-stream;base64,{}", atlas::b64::encode(&bin));
    let gltf = format!(
        r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],
        "nodes":[{{"translation":[10,0,0],"scale":[2,2,2],"children":[1]}},{{"translation":[0,5,0],"mesh":0}}],
        "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}},"indices":1,"material":0}}]}}],
        "materials":[{{"pbrMetallicRoughness":{{"baseColorFactor":[0.1,0.5,0.9,1]}}}}],
        "accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}},
                     {{"bufferView":1,"componentType":5123,"count":3,"type":"SCALAR"}}],
        "bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":36}},{{"buffer":0,"byteOffset":36,"byteLength":6}}],
        "buffers":[{{"byteLength":{},"uri":"{uri}"}}]}}"#,
        bin.len()
    );
    let mesh = atlas::meshio::read_gltf(gltf.as_bytes(), None).unwrap();
    println!("LIVE [gltf by hand] {} triangle(s), from {:?} to {:?}, colours {:?}", mesh.tris.len(), mesh.lo, mesh.hi, mesh.colours);
    assert_eq!(mesh.tris.len(), 1);
    assert_eq!(mesh.lo, [10.0, 10.0, 0.0]);
    assert_eq!(mesh.hi, [12.0, 12.0, 0.0]);
    assert_eq!(mesh.colours.len(), 1);
    assert!((mesh.colours[0][2] - 0.9).abs() < 1e-6);
}

const FRONT: &str = r#""camera":{"from":[0,0.7,5],"at":[0,0.7,0],"fov":35}"#;

#[test]
fn a_model_stands_where_its_fit_puts_it_and_blender_agrees_for_every_format() {
    let m = models();
    let dir = tmp("model-blender");
    let blender = a_blender();
    let mut lines = Vec::new();
    for f in ["suzanne.obj", "suzanne.stl", "suzanne.glb"] {
        let s = scene(
            &format!(
                r##"{{"width":160,"height":120,"quality":"good","sky":"#101010","sun":{{"from":[0.2,0.5,1],"softness":0}},{FRONT},
                  "objects":[{{"shape":"mesh","file":"{f}","fit":1.4,"at":[0,0,0],"rotate":[0,25,0],"colour":"#e0e0e0"}}]}}"##
            ),
            &m,
        );
        assert!(check_scene(&s).is_empty(), "{:?}", check_scene(&s));
        let ours = render_frame(&s, 0.0);
        let mine = mask(&ours);
        // Stands on y = 0 and is 1.4 m at its longest: its lowest pixel is
        // where y = 0 lands, worked out with a pinhole here.
        let rows: Vec<u32> = (0..ours.height).filter(|y| (0..ours.width).any(|x| mine[(*y * ours.width + x) as usize])).collect();
        let (top, bottom) = (rows[0], *rows.last().unwrap());
        let t = (35f64.to_radians() / 2.0).tan();
        // The base's nearest point (its front, about z = +0.35 before turning)
        // sits a little lower on the screen than the centre of the base.
        let y_of = |y: f64, z: f64| (1.0 - (y - 0.7) / (5.0 - z) / t) / 2.0 * 120.0;
        let floor_lo = y_of(0.0, 0.0);
        let floor_hi = y_of(0.0, 0.6);
        let mut line = format!("{f}: rows {top}..{bottom} (the floor at {floor_lo:.0}–{floor_hi:.0})");
        assert!(bottom as f64 >= floor_lo - 2.0 && bottom as f64 <= floor_hi + 2.0, "{line}");
        if let Some(b) = &blender {
            let made = make(&s, &dir, &f.replace('.', "_"), 0, Some(b)).unwrap();
            let theirs = atlas::pngcodec::read_png(&std::fs::read(made.blender.as_ref().expect("Blender rendered")).unwrap()).unwrap();
            let overlap = iou(&mine, &mask(&theirs));
            line.push_str(&format!(", silhouette overlap with Blender {:.1}%", overlap * 100.0));
            assert!(overlap > 0.9, "{line}");
        }
        lines.push(line);
    }
    println!("LIVE [models in a scene] {}", lines.join(" · "));
    if blender.is_none() {
        println!("LIVE [models in a scene] Blender comparison skipped: no Blender here");
    }
}

/// A ball in front of a wall that's red on the left, blue on the right.
fn lens(material: &str) -> Scene {
    parse_scene(&format!(
        r##"{{"width":160,"height":120,"quality":"good","denoise":false,"sky":"#101010","sun":{{"from":[0,0.3,1],"softness":0}},
          "camera":{{"from":[0,0.8,6],"at":[0,0.8,0],"fov":30}},
          "objects":[{{"shape":"box","size":[3,4,0.2],"at":[-1.5,0.8,-2],"colour":"#e02020"}},
                     {{"shape":"box","size":[3,4,0.2],"at":[1.5,0.8,-2],"colour":"#2040e0"}},
                     {{"shape":"sphere","radius":0.7,"at":[0,0.8,1],{material}}}]}}"##
    ))
    .unwrap()
}

#[test]
fn a_glass_ball_turns_the_world_behind_it_upside_down() {
    let glass = render_frame(&lens(r#""material":"glass""#), 0.0);
    let air = render_frame(&lens(r#""material":"glass","ior":1.0"#), 0.0);
    // Left of the ball's middle (the ball is centred in the picture).
    let (x, y) = (80 - 12, 60);
    let red = |p: [i32; 3]| p[0] > p[2] + 40;
    let blue = |p: [i32; 3]| p[2] > p[0] + 40;
    let (g, a) = (px(&glass, x, y), px(&air, x, y));
    println!("LIVE [glass] left of the ball's middle: {g:?} through glass (ior 1.5), {a:?} through 'glass' that doesn't bend (ior 1.0)");
    assert!(red(a), "with nothing bending, the wall behind shows as it is: {a:?}");
    assert!(blue(g), "a glass ball is a lens: left and right swap: {g:?}");
    let r = 80 + 12;
    assert!(red(px(&glass, r, y)), "{:?}", px(&glass, r, y));
}

#[test]
fn glass_lets_light_through_its_shadow_and_glow_lights_its_neighbours() {
    let shadow = |material: &str| {
        let s = parse_scene(&format!(
            r##"{{"width":120,"height":90,"quality":"good","denoise":false,"sun":{{"from":[0,1,0.0001],"softness":0}},
              "camera":{{"from":[0,3,4],"at":[0,0,0],"fov":40}},
              "objects":[{{"shape":"ground","pattern":"plain","colour":"#c0c0c0"}},
                         {{"shape":"sphere","radius":0.5,"at":[0,1.2,0],{material}}}]}}"##
        ))
        .unwrap();
        let img = render_frame(&s, 0.0);
        // The shadow falls straight down onto (0, 0, 0), which the camera
        // looks straight at: the middle of the picture.
        bright(&img, 60, 45)
    };
    let solid = shadow(r##""colour":"#808080""##);
    let glass = shadow(r#""material":"glass""#);
    let sunlit = {
        let s = parse_scene(
            r##"{"width":120,"height":90,"quality":"good","denoise":false,"sun":{"from":[0,1,0.0001],"softness":0},
              "camera":{"from":[0,3,4],"at":[0,0,0],"fov":40},
              "objects":[{"shape":"ground","pattern":"plain","colour":"#c0c0c0"},{"shape":"sphere","radius":0.1,"at":[3,0.1,0]}]}"##,
        )
        .unwrap();
        bright(&render_frame(&s, 0.0), 60, 45)
    };
    println!("LIVE [glass shadow] the ground under a ball: {solid:.0} under solid, {glass:.0} under glass, {sunlit:.0} in open sun");
    assert!(glass > solid + 40.0, "{glass} vs {solid}");
    assert!(glass < sunlit + 1.0);

    let near = |material: &str| {
        let s = parse_scene(&format!(
            r##"{{"width":120,"height":90,"quality":"good","denoise":false,"sky":"#0a0c10","sun":{{"from":[0,1,0],"strength":0}},
              "camera":{{"from":[0,2.5,4],"at":[0,0.3,0],"fov":40}},
              "objects":[{{"shape":"ground","pattern":"plain","colour":"#c0c0c0"}},
                         {{"shape":"sphere","radius":0.3,"at":[0,0.3,0],"colour":"#ffa040",{material}}}]}}"##
        ))
        .unwrap();
        let img = render_frame(&s, 0.0);
        // A band of ground in front of the ball, left and right of it.
        let mut sum = 0.0;
        for x in (20..45).chain(75..100) {
            sum += bright(&img, x, 62);
        }
        sum / 50.0
    };
    let (dark, lit) = (near(r#""material":"solid""#), near(r#""material":"glow","glow":8"#));
    println!("LIVE [glow] the ground beside a ball at night: {dark:.1} beside a plain ball, {lit:.1} beside a glowing one");
    assert!(lit > dark + 8.0, "{lit} vs {dark}");
}

/// A box face-on, 1 m square, in a pattern of 0.2 m cells.
fn pattern_box(kind: &str) -> Scene {
    parse_scene(&format!(
        r##"{{"width":160,"height":120,"quality":"good","denoise":false,"sky":"#101010","sun":{{"from":[0.2,0.4,1],"softness":0}},
          "camera":{{"from":[0,0,4],"at":[0,0,0],"fov":30}},
          "objects":[{{"shape":"box","size":[1,1,1],"at":[0,0,0],"colour":"#d02020",
                       "pattern":{{"kind":"{kind}","colour":"#f0f0f0","size":0.2}}}}]}}"##
    ))
    .unwrap()
}

fn is_red(p: [i32; 3]) -> bool {
    p[0] > p[1] + 60
}

/// The front face, in pixels: where a 1 m face at z = 0.5 lands.
fn face(img: &Rgba) -> (u32, u32, u32, u32) {
    let t = (30f64.to_radians() / 2.0).tan();
    let half = 0.5 / 3.5 / t * 60.0;
    let (x0, x1) = ((80.0 - half * 0.92) as u32, (80.0 + half * 0.92) as u32);
    let (y0, y1) = ((60.0 - half * 0.92) as u32, (60.0 + half * 0.92) as u32);
    let _ = img;
    (x0, x1, y0, y1)
}

#[test]
fn patterns_have_the_size_they_are_given_and_blender_draws_the_same_ones() {
    let mut said = Vec::new();
    for kind in ["checker", "stripes", "grid", "dots", "noise"] {
        let s = pattern_box(kind);
        let img = render_frame(&s, 0.0);
        let (x0, x1, y0, y1) = face(&img);
        let row: Vec<bool> = (x0..x1).map(|x| is_red(px(&img, x, 60 + 3))).collect();
        let col: Vec<bool> = (y0..y1).map(|y| is_red(px(&img, 80 + 3, y))).collect();
        let changes = |v: &[bool]| v.windows(2).filter(|w| w[0] != w[1]).count();
        let mut light = 0;
        let mut all = 0;
        for y in y0..y1 {
            for x in x0..x1 {
                all += 1;
                if !is_red(px(&img, x, y)) {
                    light += 1;
                }
            }
        }
        let share = light as f64 / all as f64;
        said.push(format!("{kind}: {} changes across, {} down, {:.0}% second colour", changes(&row), changes(&col), share * 100.0));
        match kind {
            // A 1 m face centred on 0 in 0.2 m squares: half a square, four
            // whole ones, half a square — 5 changes each way.
            "checker" => assert!(changes(&row) == 5 && changes(&col) == 5 && (share - 0.5).abs() < 0.12, "{said:?}"),
            // Bands up the object: none across, 5 down.
            "stripes" => assert!(changes(&row) == 0 && changes(&col) == 5, "{said:?}"),
            // Lines 0.12 of a cell wide each way (1 − 0.88² ≈ 23 % of the face,
            // more on screen where a pixel is part line, part square); five
            // lines across, so ten changes.
            "grid" => assert!(changes(&row) == 10 && changes(&col) == 10 && share > 0.18 && share < 0.40, "{said:?}"),
            // The face cuts the middle of a layer of dots of radius 0.3 cells
            // (π·0.3² ≈ 28 %, again a little more on screen).
            "dots" => assert!(share > 0.22 && share < 0.42, "{said:?}"),
            // Noise: both colours and the blends between.
            _ => assert!(share > 0.05 && share < 0.95, "{said:?}"),
        }
    }
    println!("LIVE [patterns] {}", said.join(" · "));
    let Some(blender) = a_blender() else { return println!("LIVE [patterns, Blender] skipped: no Blender here") };
    let dir = tmp("patterns-blender");
    let mut agree = Vec::new();
    for kind in ["checker", "stripes", "grid", "dots"] {
        let s = pattern_box(kind);
        let ours = render_frame(&s, 0.0);
        let made = make(&s, &dir, kind, 0, Some(&blender)).unwrap();
        let theirs = atlas::pngcodec::read_png(&std::fs::read(made.blender.as_ref().expect("Blender rendered")).unwrap()).unwrap();
        let (x0, x1, y0, y1) = face(&ours);
        let (mut same, mut n) = (0, 0);
        for y in y0..y1 {
            for x in x0..x1 {
                n += 1;
                // Blender's red is lighter under its own tone curve; "red" is the hue.
                let b = px(&theirs, x, y);
                let theirs_red = b[0] > b[1] + 40;
                if theirs_red == is_red(px(&ours, x, y)) {
                    same += 1;
                }
            }
        }
        let k = same as f64 / n as f64;
        agree.push(format!("{kind} {:.1}%", k * 100.0));
        assert!(k > 0.9, "{kind}: {agree:?}");
    }
    println!("LIVE [patterns, Blender] the same pixel is the same colour of the pattern in Atlas and Blender: {}", agree.join(", "));
}

/// Night, a big soft moon, and a lamp on the floor: the noisiest light
/// there is to draw.
fn night(quality: &str, denoise: bool) -> Scene {
    parse_scene(&format!(
        r##"{{"width":160,"height":100,"quality":"{quality}","denoise":{denoise},"sky":"#0a0c12",
          "sun":{{"from":[-0.6,1,0.5],"softness":30,"strength":0.25}},
          "camera":{{"from":[0,1.4,4.5],"at":[0,0.5,0],"fov":40}},
          "objects":[{{"shape":"ground","colour":"#d8d0c0"}},
                     {{"shape":"sphere","radius":0.25,"at":[0.1,0.25,0.6],"material":"glow","colour":"#ffb050","glow":10}},
                     {{"shape":"sphere","radius":0.5,"at":[-0.7,0.5,0],"colour":"#d9730d"}},
                     {{"shape":"box","size":[0.6,0.6,0.6],"at":[0.9,0.3,-0.3],"rotate":[0,30,0],"colour":"#3060c0"}}]}}"##
    ))
    .unwrap()
}

fn rmse(a: &Rgba, b: &Rgba) -> f64 {
    let n = a.pixels.len();
    let s: f64 = (0..n).filter(|i| i % 4 != 3).map(|i| (a.pixels[i] as f64 - b.pixels[i] as f64).powi(2)).sum();
    (s / (n as f64 * 0.75)).sqrt()
}

/// The strength of edges where the reference has them.
fn edges(img: &Rgba, at: &[(u32, u32)]) -> f64 {
    at.iter().map(|&(x, y)| (bright(img, x + 1, y) - bright(img, x, y)).abs()).sum::<f64>() / at.len().max(1) as f64
}

#[test]
fn the_denoiser_takes_grain_out_and_leaves_edges_in() {
    let reference = render_frame(&night("reference", false), 0.0);
    let raw = render_frame(&night("draft", false), 0.0);
    let started = std::time::Instant::now();
    let smooth = render_frame(&night("draft", true), 0.0);
    let took = started.elapsed().as_secs_f64();
    let (e_raw, e_smooth) = (rmse(&raw, &reference), rmse(&smooth, &reference));
    // Where the reference has a real edge (an outline, a shadow's edge).
    let mut at = Vec::new();
    for y in 1..reference.height - 1 {
        for x in 1..reference.width - 2 {
            if (bright(&reference, x + 1, y) - bright(&reference, x, y)).abs() > 30.0 {
                at.push((x, y));
            }
        }
    }
    let (kept_ref, kept) = (edges(&reference, &at), edges(&smooth, &at));
    println!(
        "LIVE [denoiser] draft (4 rays a pixel) against a 64-ray reference: off by {e_raw:.2} levels raw, {e_smooth:.2} denoised; \
         edges kept {:.0}% of their strength over {} edge pixels; draft + denoise {took:.1} s",
        kept / kept_ref * 100.0,
        at.len()
    );
    // Fireflies: pixels far brighter than they should be. Aiming at the lamp
    // (rather than hoping a sky ray finds it) is what keeps these out.
    let fireflies = (0..(raw.width * raw.height))
        .filter(|i| {
            let (x, y) = (i % raw.width, i / raw.width);
            bright(&raw, x, y) > bright(&reference, x, y) + 60.0
        })
        .count();
    println!("LIVE [lamp light] pixels 60+ levels too bright in the raw draft: {fireflies} of {}", raw.width * raw.height);
    assert!(fireflies < 20, "{fireflies}");
    // Measured on this scene: 3.31 → 2.91; on a held-out scene (glass,
    // lamp, model, patterns) 3.19 → 3.01. Modest, and said so.
    assert!(e_smooth < e_raw * 0.95, "{e_smooth} vs {e_raw}");
    assert!(kept > kept_ref * 0.7, "{kept} vs {kept_ref}");
}

#[test]
fn a_missing_model_or_unknown_pattern_is_said_before_drawing() {
    let s = parse_scene(&format!(
        r##"{{{FRONT},"objects":[{{"shape":"mesh","file":"no-such-thing.glb","fit":1}},
            {{"shape":"sphere","radius":0.3,"pattern":"paisley"}}]}}"##
    ))
    .unwrap();
    let found = check_scene(&s);
    println!("LIVE [checks] {}", found.iter().map(|f| f.detail.clone()).collect::<Vec<_>>().join(" · "));
    assert!(found.iter().any(|f| f.rule == "models read" && f.severity == atlas::motion::Severity::Blocking), "{found:?}");
    assert!(found.iter().any(|f| f.rule == "patterns known"), "{found:?}");
    // Drawing anyway leaves the missing model out rather than failing.
    let img = render_frame(&s, 0.0);
    assert!(mask(&img).iter().any(|m| *m));
}

#[test]
fn a_scene_file_finds_its_models_beside_it_and_the_folder_is_listed() {
    let dir = tmp("beside");
    std::fs::copy(models().join("suzanne.stl"), dir.join("head.stl")).unwrap();
    std::fs::write(dir.join("notes.txt"), "not a model").unwrap();
    std::fs::write(
        dir.join("look.json"),
        format!(r##"{{"width":64,"height":48,"quality":"draft","sky":"#101010",{FRONT},"objects":[{{"shape":"mesh","file":"head.stl","fit":2}}]}}"##),
    )
    .unwrap();
    let s = load_scene(&dir.join("look.json")).unwrap();
    assert!(check_scene(&s).is_empty(), "{:?}", check_scene(&s));
    assert!(mask(&render_frame(&s, 0.0)).iter().filter(|m| **m).count() > 60);
    assert_eq!(atlas::scene3d::model_files(&dir), vec!["head.stl".to_string()]);
}

#[test]
fn the_blender_script_imports_models_and_makes_glass_glow_and_patterns() {
    let s = scene(
        &format!(
            r##"{{{FRONT},"objects":[{{"shape":"mesh","file":"suzanne.obj","fit":1}},{{"shape":"mesh","file":"suzanne.stl","fit":1}},
                {{"shape":"mesh","file":"suzanne.glb","fit":1,"material":"glass"}},{{"shape":"sphere","radius":0.2,"material":"glow","glow":5}},
                {{"shape":"box","size":[1,1,1],"pattern":"dots"}}]}}"##
        ),
        &models(),
    );
    let py = atlas::scene3d::blender_script(&s, Path::new("/tmp/out.png"));
    for want in [
        "bpy.ops.wm.obj_import",
        "bpy.ops.wm.stl_import",
        "bpy.ops.import_scene.gltf",
        "'Transmission Weight'",
        "'Emission Strength'",
        "pat=('dots'",
        "keep=True",
        "os._exit(0)",
    ] {
        assert!(py.contains(want), "{want} missing");
    }
    // The OBJ keeps its own gold; the glass one is repainted.
    assert!(py.contains("ior=1.5000"), "glass");
}
