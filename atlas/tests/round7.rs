//! Round 7: 3-D that moves. Keyframes, easing, spin, a camera that travels,
//! motion blur, soft shadows — measured on the pictures themselves
//! (`cargo test --test all round7 -- --nocapture`).
//!
//! Where a test checks *where* something was drawn, the expected spot is
//! worked out here with its own pinhole-camera arithmetic, not by asking the
//! renderer — so the renderer can't pass by agreeing with itself.

use atlas::pngcodec::Rgba;
use atlas::scene3d::{make, parse_scene, render_frame, Scene};
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-r7-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn have(tool: &str) -> bool {
    atlas::tools::which(tool).is_some()
        && std::process::Command::new(tool).arg("-version").output().map(|o| o.status.success()).unwrap_or(false)
}

fn ffprobe_frames(file: &Path) -> String {
    let out = std::process::Command::new("ffprobe")
        .args(["-v", "error", "-count_frames", "-select_streams", "v:0", "-show_entries", "stream=width,height,nb_read_frames", "-of", "csv=p=0"])
        .arg(file)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Pixels that are clearly this hue: (r, g, b) → bool.
fn centre_of(img: &Rgba, want: impl Fn(i32, i32, i32) -> bool) -> Option<(f64, f64, usize)> {
    let (mut sx, mut sy, mut n) = (0.0, 0.0, 0usize);
    for y in 0..img.height {
        for x in 0..img.width {
            let p = img.at(x, y);
            if want(p[0] as i32, p[1] as i32, p[2] as i32) {
                sx += x as f64;
                sy += y as f64;
                n += 1;
            }
        }
    }
    (n > 0).then(|| (sx / n as f64, sy / n as f64, n))
}

fn orange(r: i32, g: i32, b: i32) -> bool {
    r > g + 40 && g > b + 20
}

/// Where a point lands for a still camera — worked out here, independently.
fn pinhole(from: [f64; 3], at: [f64; 3], fov: f64, w: f64, h: f64, p: [f64; 3]) -> (f64, f64) {
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let norm = |a: [f64; 3]| {
        let l = dot(a, a).sqrt();
        [a[0] / l, a[1] / l, a[2] / l]
    };
    let f = norm(sub(at, from));
    let r = norm([-f[2], 0.0, f[0]]); // f × up, up = y
    let u = [r[1] * f[2] - r[2] * f[1], r[2] * f[0] - r[0] * f[2], r[0] * f[1] - r[1] * f[0]];
    let rel = sub(p, from);
    let z = dot(rel, f);
    let t = (fov.to_radians() / 2.0).tan();
    let x = dot(rel, r) / z / (t * w / h);
    let y = dot(rel, u) / z / t;
    ((x + 1.0) / 2.0 * w, (1.0 - y) / 2.0 * h)
}

const CAM: &str = r#""camera":{"from":[0,1.2,6],"at":[0,0.8,0],"fov":40}"#;

fn rolling_ball(blur: bool) -> Scene {
    parse_scene(&format!(
        r##"{{"width":160,"height":96,"quality":"draft","motion_blur":{blur},"duration":1,"fps":8,{CAM},
            "objects":[{{"shape":"ground","y":0,"colour":"#d8d2c4","pattern":"plain"}},
                       {{"shape":"sphere","radius":0.35,"colour":"#d9730d",
                         "animate":[{{"prop":"at","keys":[[0,[-1.6,0.35,0]],[1,[1.6,0.35,0]]],"ease":"linear"}}]}}]}}"##
    ))
    .unwrap()
}

#[test]
fn a_keyed_ball_is_drawn_where_its_keyframes_put_it_every_frame() {
    let s = rolling_ball(false);
    let mut worst: f64 = 0.0;
    let mut line = Vec::new();
    for i in 0..8 {
        let t = i as f64 / 8.0;
        let img = render_frame(&s, t);
        let (cx, _, _) = centre_of(&img, orange).expect("the ball is drawn");
        let x = -1.6 + 3.2 * t;
        let (ex, _) = pinhole([0.0, 1.2, 6.0], [0.0, 0.8, 0.0], 40.0, 160.0, 96.0, [x, 0.35, 0.0]);
        worst = worst.max((cx - ex).abs());
        line.push(format!("{cx:.0}"));
    }
    println!("LIVE [3-D keyframes] ball centre by frame (px): {} — worst off the keyed path by {worst:.1} px", line.join(" → "));
    assert!(worst < 2.0, "{worst}");
}

#[test]
fn a_spinning_box_turns_its_silhouette() {
    let s = parse_scene(&format!(
        r##"{{"width":128,"height":96,"quality":"draft","motion_blur":false,"duration":1,"fps":4,{CAM},
            "objects":[{{"shape":"box","at":[0,0.8,0],"size":[1.6,0.8,0.3],"colour":"#3a6fc4","spin":[0,90,0]}}]}}"##
    ))
    .unwrap();
    let widths: Vec<u32> = (0..4)
        .map(|i| {
            let img = render_frame(&s, i as f64 / 4.0);
            let cols: Vec<u32> = (0..img.width)
                .filter(|&x| (0..img.height).any(|y| {
                    let p = img.at(x, y);
                    p[2] as i32 > p[0] as i32 + 40
                }))
                .collect();
            cols.len() as u32
        })
        .collect();
    println!("LIVE [3-D spin] box width on screen at 0°, 22.5°, 45°, 67.5°: {widths:?} px");
    // Face-on it is widest; turned 67.5° its long side is foreshortened.
    assert!(widths[0] > widths[3] + 8, "{widths:?}");
}

#[test]
fn a_bounce_lands_and_settles_on_the_ground() {
    let s = parse_scene(&format!(
        r##"{{"width":128,"height":96,"quality":"draft","motion_blur":false,"duration":1,"fps":10,{CAM},
            "objects":[{{"shape":"sphere","radius":0.3,"colour":"#d9730d",
                         "animate":[{{"prop":"at","keys":[[0,[0,2.0,0]],[1,[0,0.3,0]]],"ease":"bounce"}}]}}]}}"##
    ))
    .unwrap();
    let ys: Vec<f64> = (0..10).map(|i| centre_of(&render_frame(&s, i as f64 / 10.0), orange).unwrap().1).collect();
    let end = centre_of(&render_frame(&s, 1.0), orange).unwrap().1;
    let (_, landed) = pinhole([0.0, 1.2, 6.0], [0.0, 0.8, 0.0], 40.0, 128.0, 96.0, [0.0, 0.3, 0.0]);
    println!("LIVE [3-D bounce] ball height on screen by frame (px, down is bigger): {:?} → rests at {end:.0} (keyed {landed:.0})", ys.iter().map(|y| y.round()).collect::<Vec<_>>());
    // It falls, then — the bounce — goes back up (smaller y) before settling.
    let rose = ys.windows(2).position(|w| w[1] < w[0] - 2.0);
    assert!(rose.is_some_and(|i| i > 0 && ys[i] > ys[0] + 20.0), "no bounce after the fall: {ys:?}");
    assert!((end - landed).abs() < 2.0);
}

#[test]
fn motion_blur_smears_what_moves_fast_and_nothing_else() {
    let at = |blur: bool, with_ball: bool| {
        let mut s = rolling_ball(blur);
        s.fps = 4; // 0.8 m a frame: fast
        s.quality = atlas::scene3d::Quality::Good;
        if !with_ball {
            s.objects.truncate(1);
        }
        render_frame(&s, 0.5)
    };
    let empty = at(false, false);
    // Columns where the ball changed the picture at all (by more than 6 levels).
    let touched = |img: &Rgba| {
        (0..img.width)
            .filter(|&x| (0..img.height).any(|y| {
                let (p, q) = (img.at(x, y), empty.at(x, y));
                (0..3).any(|c| (p[c] as i32 - q[c] as i32).abs() > 6)
            }))
            .count()
    };
    let (sharp, blurred) = (touched(&at(false, true)), touched(&at(true, true)));
    // Half a frame at 4 fps is 0.4 m of travel; at 6 m through a 40° lens on
    // 96 rows that's about 0.4 × 96 / (2·6·tan 20°) ≈ 8.8 px more.
    let expect = 0.4 * 96.0 / (2.0 * 6.0 * 20f64.to_radians().tan());
    println!("LIVE [3-D motion blur] the moving ball touches {sharp} columns sharp, {blurred} with the shutter open half a frame (travel in that time ≈ {expect:.1} px)");
    assert!((blurred as f64 - sharp as f64) > expect * 0.5, "{sharp} vs {blurred}");
    // Something that isn't moving isn't blurred: with the ball gone, the open
    // shutter changes not one pixel.
    assert_eq!(at(true, false).pixels, empty.pixels);
}

#[test]
fn a_bigger_sun_gives_softer_shadow_edges() {
    let scene = |soft: f64| {
        parse_scene(&format!(
            r##"{{"width":160,"height":96,"quality":"good","sun":{{"from":[-0.4,1,0.2],"softness":{soft}}},
                "camera":{{"from":[0,4,4],"at":[0,0,0],"fov":40}},
                "objects":[{{"shape":"ground","y":0,"colour":"#dddddd","pattern":"plain"}},
                           {{"shape":"box","at":[0,1.2,0],"size":[0.6,0.2,0.6],"colour":"#dddddd"}}]}}"##
        ))
        .unwrap()
    };
    // Along the row through the middle of the shadow, how many pixels it takes
    // to go from shadow to full light: the width of the soft edge.
    let edge = |img: &Rgba| {
        let lum = |x: u32, y: u32| {
            let p = img.at(x, y);
            p[0] as i32 + p[1] as i32 + p[2] as i32
        };
        // The darkest row on the ground is through the shadow.
        let (mut best_y, mut darkest) = (0, i32::MAX);
        for y in img.height / 2..img.height {
            let m = (0..img.width).map(|x| lum(x, y)).min().unwrap();
            if m < darkest {
                darkest = m;
                best_y = y;
            }
        }
        let row: Vec<i32> = (0..img.width).map(|x| lum(x, best_y)).collect();
        let (lo, hi) = (*row.iter().min().unwrap(), *row.iter().max().unwrap());
        let (a, b) = (lo + (hi - lo) / 10, hi - (hi - lo) / 10);
        row.iter().filter(|l| **l > a && **l < b).count()
    };
    let (hard, soft) = (edge(&render_frame(&scene(0.0), 0.0)), edge(&render_frame(&scene(12.0), 0.0)));
    // The box's underside is 1.1 m up; a 12° sun (6° either side) spreads
    // each edge over 1.1 × 2·tan 6° ≈ 0.23 m of ground — two edges a row.
    println!("LIVE [3-D soft shadows] pixels across the shadow's edges on one row: {hard} with a point sun, {soft} with a 12° sun");
    assert!(soft >= hard + 4, "{hard} vs {soft}");
}

#[test]
fn a_moving_scene_comes_out_as_a_gif_and_an_mp4_with_every_frame() {
    let dir = tmp("film");
    let s = parse_scene(&format!(
        r##"{{"width":160,"height":96,"quality":"draft","duration":1.5,"fps":12,
            "camera":{{"from":[0,1.6,6],"at":[0,0.6,0],"fov":40,"orbit":40}},
            "objects":[{{"shape":"ground","y":0,"colour":"#d8d2c4"}},
                       {{"shape":"torus","at":[0,0.7,0],"radius":0.6,"thickness":0.18,"colour":"#d4a52c","shine":0.6,"spin":[120,60,0]}},
                       {{"shape":"sphere","radius":0.25,"colour":"#d9730d",
                         "animate":[{{"prop":"at","keys":[[0,[-1.4,0.25,0.8]],[0.75,[1.4,0.25,0.8]],[1.5,[-1.4,0.25,0.8]]],"ease":"ease-in-out"}},
                                    {{"prop":"colour","keys":[[0,"#d9730d"],[1.5,"#3a6fc4"]],"ease":"linear"}}]}}]}}"##
    ))
    .unwrap();
    let made = make(&s, &dir, "rings", 0, None).unwrap();
    println!("LIVE [3-D animation] {}", made.say().replace('\n', " "));
    assert!(atlas::motion::blocking(&made.findings).is_empty(), "{:?}", made.findings);
    assert_eq!(made.frames, 18);
    assert_eq!(made.changed, 17, "every frame should differ from the one before");
    if have("ffmpeg") {
        let gif = ffprobe_frames(made.gif.as_ref().unwrap());
        let mp4 = ffprobe_frames(made.mp4.as_ref().unwrap());
        println!("LIVE [3-D animation] ffprobe: GIF {gif}, MP4 {mp4} (width,height,frames)");
        assert_eq!(gif, "160,96,18");
        assert_eq!(mp4, "160,96,18");
    }
}

#[test]
fn leaving_the_frame_and_not_moving_at_all_are_both_said() {
    let off = parse_scene(&format!(
        r##"{{"width":96,"height":64,"quality":"draft","duration":2,"fps":8,{CAM},
            "objects":[{{"shape":"sphere","radius":0.3,"colour":"#d9730d",
                         "animate":[{{"prop":"at","keys":[[0,[0,0.8,0]],[2,[12,0.8,0]]],"ease":"linear"}}]}}]}}"##
    ))
    .unwrap();
    let said: Vec<String> = atlas::scene3d::check_scene(&off).into_iter().map(|f| f.detail).collect();
    println!("LIVE [3-D checks] a ball keyed off to the right: {said:?}");
    assert!(said.iter().any(|d| d.contains("out of the picture from about")), "{said:?}");

    let still = parse_scene(&format!(r##"{{"duration":2,{CAM},"objects":[{{"shape":"sphere","radius":0.3,"at":[0,0.8,0]}}]}}"##)).unwrap();
    let said: Vec<String> = atlas::scene3d::check_scene(&still).into_iter().map(|f| f.detail).collect();
    println!("LIVE [3-D checks] 2 s long with nothing animated: {said:?}");
    assert!(said.iter().any(|d| d.contains("nothing in it is animated")));
}

#[test]
fn the_blender_script_bakes_the_same_motion_frame_by_frame() {
    if !(atlas::tools::which("python3").is_some()
        && std::process::Command::new("python3").arg("--version").output().map(|o| o.status.success()).unwrap_or(false))
    {
        return println!("LIVE [3-D blender] skipped: no working python3 here");
    }
    let dir = tmp("blend");
    let s = rolling_ball(true);
    let script = atlas::scene3d::blender_script(&s, &dir.join("ball."));
    let path = dir.join("anim.py");
    std::fs::write(&path, &script).unwrap();
    let st = std::process::Command::new("python3").args(["-m", "py_compile"]).arg(&path).status().unwrap();
    assert!(st.success(), "the Blender script doesn't compile");
    assert!(script.contains("scene.frame_end = 8"));
    assert!(script.contains("bpy.ops.render.render(animation=True)"));
    // The ball's keyed x at frame 5 (t = 0.5) is 0: baked, not re-derived by Blender.
    assert!(script.contains("(5, (0.00000, -0.00000, 0.35000)"), "frame 5 should put the ball at the middle");
    println!("LIVE [3-D blender] {} lines, compiles; 8 frames baked for the ball and the camera; motion blur on; renders frames ball.0001.png…", script.lines().count());
}

#[test]
fn ordered_dithering_keeps_a_smooth_gradient_smooth() {
    if !have("ffmpeg") {
        return println!("LIVE [gif dither] skipped: no ffmpeg");
    }
    let dir = tmp("dither");
    // A sky-like gradient with a thousand shades in it, more than a GIF holds.
    let mut img = Rgba::new(256, 64);
    for y in 0..64 {
        for x in 0..256u32 {
            img.put(x, y, [(150 + x / 4) as u8, (170 + x / 6 + y / 8) as u8, (210 + x / 10) as u8, 255]);
        }
    }
    let err = |dither: bool| {
        let f = [atlas::gifenc::Frame { image: &img, delay_cs: 10 }];
        let bytes = if dither { atlas::gifenc::encode_gif_dithered(&f).unwrap() } else { atlas::gifenc::encode_gif(&f).unwrap() };
        let p = dir.join(if dither { "d.gif" } else { "p.gif" });
        std::fs::write(&p, bytes).unwrap();
        let out = std::process::Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-i"])
            .arg(&p)
            .args(["-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
            .output()
            .unwrap()
            .stdout;
        // Seen from a step back: 8×8 blocks averaged, compared with the source.
        let mut total = 0.0;
        for by in 0..8 {
            for bx in 0..32 {
                for c in 0..3 {
                    let (mut a, mut b) = (0.0, 0.0);
                    for y in by * 8..by * 8 + 8 {
                        for x in bx * 8..bx * 8 + 8 {
                            a += out[(y * 256 + x) * 3 + c] as f64;
                            b += img.at(x as u32, y as u32)[c] as f64;
                        }
                    }
                    total += ((a - b) / 64.0).abs();
                }
            }
        }
        total / (8.0 * 32.0 * 3.0)
    };
    let (plain, dithered) = (err(false), err(true));
    println!("LIVE [gif dither] a gradient of more shades than a GIF holds, averaged over 8×8 blocks: off by {plain:.3} levels plain, {dithered:.3} dithered");
    assert!(dithered <= plain + 0.05, "{plain} vs {dithered}");
}

struct Director(std::sync::Mutex<usize>);
impl atlas::brain::Llm for Director {
    fn complete(&self, system: &str, _user: &str) -> atlas::error::Result<String> {
        if system == atlas::scene3d::SCENE_SYSTEM {
            let mut n = self.0.lock().unwrap();
            *n += 1;
            // First a still scene (the mistake), then the moving one.
            let still = *n == 1;
            return Ok(format!(
                "```json\n{{\"width\":128,\"height\":80,\"quality\":\"draft\",\"duration\":{},\"fps\":8,{CAM},\
                 \"objects\":[{{\"shape\":\"ground\",\"y\":0}},{{\"shape\":\"sphere\",\"radius\":0.3,\"colour\":\"#d9730d\",\
                 \"animate\":[{{\"prop\":\"at\",\"keys\":[[0,[0,1.6,0]],[1,[0,0.3,0]]],\"ease\":\"bounce\"}}]}}]}}\n```",
                if still { 0 } else { 1 }
            ));
        }
        Ok("{\"action\":\"say\",\"arg\":null,\"say\":\"?\"}".into())
    }
}

#[test]
fn asking_for_a_bouncing_ball_in_3d_gets_an_animation() {
    let c = atlas::config::Config::load(Path::new("config")).unwrap();
    let p = atlas::platform::mock::MockPlatform::new(vec![atlas::platform::Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let llm = std::sync::Arc::new(Director(std::sync::Mutex::new(0)));
    let mut d = atlas::daemon::Daemon::new(
        &c,
        &p,
        Some(llm.clone()),
        atlas::store::Store::new(tmp("turn")),
        atlas::proactive::Proactive::new(atlas::proactive::ProactiveConfig::default()),
    );
    let said = d.turn("animate a bouncing ball in 3d", 1_790_000_000);
    println!("LIVE [you: \"animate a bouncing ball in 3d\"]  Atlas: {}", said.replace('\n', " "));
    assert!(said.contains("Animated it"), "{said}");
    assert_eq!(*llm.0.lock().unwrap(), 2, "the still draft should have been sent back once");
}

/// Blender, when there is one: the installed program, or — in the container
/// these tests were written in — Blender 4.2 as a Python module (the official
/// `bpy` wheel) behind a two-line `blender` wrapper.
fn a_blender() -> Option<PathBuf> {
    atlas::scene3d::find_blender(None).or_else(|| {
        let p = PathBuf::from("/tmp/claude-0/bin/blender");
        p.exists().then_some(p)
    })
}

#[test]
fn blender_renders_the_same_motion_the_in_house_renderer_draws() {
    let Some(blender) = a_blender() else { return println!("LIVE [3-D blender, animated] skipped: no Blender here") };
    let dir = tmp("blender-anim");
    let s = parse_scene(&format!(
        r##"{{"width":160,"height":96,"quality":"draft","duration":1,"fps":6,"motion_blur":false,{CAM},
            "objects":[{{"shape":"ground","y":0,"colour":"#d8d2c4"}},
                       {{"shape":"sphere","radius":0.35,"colour":"#d9730d",
                         "animate":[{{"prop":"at","keys":[[0,[-1.6,0.35,0]],[1,[1.6,0.35,0]]],"ease":"ease-in-out"}}]}}]}}"##
    ))
    .unwrap();
    let started = std::time::Instant::now();
    let made = make(&s, &dir, "roll", 0, Some(&blender)).unwrap();
    println!("LIVE [3-D blender, animated] {} ({:.1} s with Blender)", made.say().replace('\n', " "), started.elapsed().as_secs_f64());
    assert!(made.blender.is_some(), "{:?}", made.notes);
    assert!(atlas::motion::blocking(&made.findings).is_empty(), "{:?}", made.findings);
    // Frame by frame: the ball's centre in Blender's render against the
    // in-house one. Two different renderers, the same baked timeline.
    let mut worst: f64 = 0.0;
    let mut pairs = Vec::new();
    for f in 0..6 {
        let theirs = atlas::pngcodec::read_png(&std::fs::read(dir.join(format!("roll.blender.{:04}.png", f + 1))).unwrap()).unwrap();
        let ours = render_frame(&s, f as f64 / 6.0);
        let (bx, _, _) = centre_of(&theirs, orange).expect("Blender drew the ball");
        let (ox, _, _) = centre_of(&ours, orange).expect("Atlas drew the ball");
        worst = worst.max((bx - ox).abs());
        pairs.push(format!("{ox:.0}/{bx:.0}"));
    }
    println!("LIVE [3-D blender, animated] ball x per frame, Atlas/Blender: {} — worst apart {worst:.1} px", pairs.join(" "));
    assert!(worst < 2.5, "{worst}");
}
