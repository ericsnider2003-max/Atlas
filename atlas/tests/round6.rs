//! Round 6: the deferred rendering arms (B4) and the second look at who's who,
//! each driven through the part of Atlas that uses it
//! (`cargo test --test all round6 -- --nocapture`). The diarization
//! measurement is in the optimised voice target:
//! `cargo test --release --test voice_measured -- --nocapture`.
//!
//! What checks what: Atlas's PNG and GIF are read back by ffmpeg — a decoder
//! Atlas didn't write — so a round trip can't pass by agreeing with itself.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::pngcodec::{read_png, write_png, Rgba};
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-r6-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// Is the tool really here? Asked by running it, not by finding a file:
/// Windows puts a `python3.exe` on the PATH that only opens the Microsoft
/// Store (found running these tests on the laptop, 24 Sep).
fn have(tool: &str) -> bool {
    atlas::tools::which(tool).is_some()
        && std::process::Command::new(tool).arg("-version").output().map(|o| o.status.success()).unwrap_or(false)
            | std::process::Command::new(tool).arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

/// A browser to play animations in: the one Atlas finds, else the Chromium
/// this container's test harness carries.
fn a_browser() -> Option<PathBuf> {
    atlas::filmstrip::find_browser(None).or_else(|| {
        let p = PathBuf::from("/opt/pw-browsers/chromium");
        p.exists().then_some(p)
    })
}

/// Decode any image or video to raw RGB frames with ffmpeg.
fn ffmpeg_frames(file: &Path, w: u32, h: u32) -> Vec<Vec<u8>> {
    let out = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(file)
        .args(["-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .output()
        .unwrap();
    assert!(out.status.success(), "ffmpeg couldn't read {}: {}", file.display(), String::from_utf8_lossy(&out.stderr));
    out.stdout.chunks((w * h * 3) as usize).map(|c| c.to_vec()).collect()
}

fn centre_of(img: &Rgba, want: impl Fn([u8; 4]) -> bool) -> Option<(f32, f32)> {
    let (mut sx, mut sy, mut n) = (0f32, 0f32, 0f32);
    for y in 0..img.height {
        for x in 0..img.width {
            if want(img.at(x, y)) {
                sx += x as f32;
                sy += y as f32;
                n += 1.0;
            }
        }
    }
    (n > 0.0).then(|| (sx / n, sy / n))
}

#[test]
fn atlas_png_is_read_by_ffmpeg_and_ffmpeg_png_is_read_by_atlas() {
    if !have("ffmpeg") {
        return println!("LIVE [png] skipped: no ffmpeg here");
    }
    let dir = tmp("png");
    let mut img = Rgba::new(97, 41);
    for y in 0..41 {
        for x in 0..97 {
            img.put(x, y, [(x * 2) as u8, (y * 6) as u8, ((x + y) * 3) as u8, 255]);
        }
    }
    let ours = dir.join("ours.png");
    std::fs::write(&ours, write_png(&img)).unwrap();
    let back = &ffmpeg_frames(&ours, 97, 41)[0];
    let want: Vec<u8> = img.pixels.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    assert_eq!(back, &want, "ffmpeg read Atlas's PNG differently");

    // The other way: ffmpeg writes a compressed, filtered PNG (the kind a
    // browser screenshot is) and Atlas reads it exactly.
    let theirs = dir.join("theirs.png");
    let st = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc2=s=160x90:d=1", "-frames:v", "1"])
        .arg(&theirs)
        .status()
        .unwrap();
    assert!(st.success());
    let read = read_png(&std::fs::read(&theirs).unwrap()).unwrap();
    let truth = &ffmpeg_frames(&theirs, 160, 90)[0];
    let mine: Vec<u8> = read.pixels.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    assert_eq!(&mine, truth, "Atlas read ffmpeg's PNG differently");
    println!("LIVE [png] 97×41 written by Atlas, read by ffmpeg: identical. 160×90 test card written by ffmpeg ({} bytes, compressed and filtered), read by Atlas: identical.", std::fs::metadata(&theirs).unwrap().len());
}

#[test]
fn a_gif_of_a_moving_square_plays_back_in_ffmpeg_frame_for_frame() {
    if !have("ffmpeg") {
        return println!("LIVE [gif] skipped: no ffmpeg here");
    }
    let dir = tmp("gif");
    let frames: Vec<Rgba> = (0..10)
        .map(|i| {
            let mut f = Rgba::new(120, 60);
            for y in 0..60 {
                for x in 0..120 {
                    f.put(x, y, [240, 236, 228, 255]);
                }
            }
            for y in 20..40 {
                for x in i * 10..i * 10 + 20 {
                    f.put(x, y, [217, 115, 13, 255]);
                }
            }
            f
        })
        .collect();
    let gf: Vec<atlas::gifenc::Frame> = frames.iter().map(|f| atlas::gifenc::Frame { image: f, delay_cs: 10 }).collect();
    let bytes = atlas::gifenc::encode_gif(&gf).unwrap();
    let path = dir.join("square.gif");
    std::fs::write(&path, &bytes).unwrap();
    let back = ffmpeg_frames(&path, 120, 60);
    assert_eq!(back.len(), 10, "ffmpeg should see ten frames");
    for (i, (b, f)) in back.iter().zip(&frames).enumerate() {
        let want: Vec<u8> = f.pixels.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
        assert_eq!(b, &want, "frame {i} came back different");
    }
    let raw = 120 * 60 * 3 * 10;
    println!(
        "LIVE [gif] 10 frames of an orange square crossing a cream ground: {} bytes as a GIF ({} raw), ffmpeg plays back all 10 pixel-for-pixel. Only the changed rectangle is stored after frame 1.",
        bytes.len(),
        raw
    );
    assert!(bytes.len() < raw / 20);
}

const BALL: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
<title>a ball rolling right</title>
<rect width="200" height="100" fill="#ffffff"/>
<circle cx="20" cy="60" r="12" fill="#ff0000"><animate attributeName="cx" from="20" to="180" dur="2s" repeatCount="indefinite"/></circle>
<rect x="0" y="0" width="20" height="20" fill="#0000ff" style="animation:slide 2s linear infinite"/>
<style>@keyframes slide{from{transform:translateX(0)}to{transform:translateX(180px)}}</style>
</svg>"##;

#[test]
fn an_svg_animation_is_played_to_a_gif_and_each_frame_is_the_moment_asked_for() {
    let Some(browser) = a_browser() else { return println!("LIVE [filmstrip] skipped: no browser here") };
    let dir = tmp("film");
    let spec = atlas::motion::MotionSpec { idea: String::new(), width: 200, height: 100, duration_secs: 0.0 };
    let plan = atlas::filmstrip::Plan::for_svg(BALL, &spec, 10);
    assert_eq!(plan.seconds, 2.0, "the duration comes from the SVG's own dur");
    let frames = atlas::filmstrip::play_frames(BALL, &plan, &browser, &dir.join("work")).unwrap();
    assert_eq!(frames.len(), 20);
    // SMIL: the red ball's centre moves 80 px a second from x = 20.
    // CSS: the blue square's left edge moves 90 px a second from 0.
    let mut worst = 0f32;
    for (i, f) in frames.iter().enumerate() {
        let t = i as f32 / 10.0;
        let (rx, _) = centre_of(f, |p| p[0] > 200 && p[1] < 60 && p[2] < 60).expect("the ball is drawn");
        let (bx, _) = centre_of(f, |p| p[2] > 200 && p[0] < 60 && p[1] < 60).expect("the square is drawn");
        worst = worst.max((rx - (20.0 + 80.0 * t)).abs()).max((bx - (9.5 + 90.0 * t)).abs());
    }
    println!("LIVE [filmstrip] 20 frames at 10 fps; ball (SMIL) and square (CSS) are where the animation says at every frame, worst off by {worst:.1} px");
    assert!(worst < 2.0, "a frame wasn't the moment asked for: {worst}");

    // The whole thing: GIF (and MP4 when ffmpeg is here), checked.
    let made = atlas::filmstrip::film(BALL, &plan, &browser, Some("ffmpeg"), &dir, "ball").unwrap();
    println!("LIVE [filmstrip] {}", made.say().replace('\n', " "));
    assert!(atlas::motion::blocking(&made.findings).is_empty(), "{:?}", made.findings);
    assert_eq!(made.distinct, 20);
    let gif = made.gif.unwrap();
    if have("ffmpeg") {
        assert_eq!(ffmpeg_frames(&gif, 200, 100).len(), 20, "ffmpeg should play 20 frames of the GIF");
        let mp4 = made.mp4.expect("ffmpeg is here, so there's an MP4");
        let probe = std::process::Command::new("ffprobe")
            .args(["-v", "error", "-count_frames", "-select_streams", "v:0", "-show_entries", "stream=width,height,nb_read_frames", "-of", "csv=p=0"])
            .arg(&mp4)
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&probe.stdout).trim().to_string();
        println!("LIVE [filmstrip] ffprobe on the MP4: {said} (width,height,frames)");
        assert_eq!(said, "200,100,20");
    }
}

#[test]
fn an_animation_that_never_moves_when_played_is_caught() {
    let Some(browser) = a_browser() else { return println!("LIVE [filmstrip] skipped: no browser here") };
    let dir = tmp("still");
    // It passes the source check — it has an <animate> with a dur — but it
    // animates to the value it already has, so nothing on screen changes.
    let still = r##"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="80"><title>nothing</title><rect width="80" height="80" fill="#fff"/><circle cx="40" cy="40" r="10" fill="#333"><animate attributeName="cx" from="40" to="40" dur="1s" repeatCount="indefinite"/></circle></svg>"##;
    let spec = atlas::motion::MotionSpec { idea: String::new(), width: 80, height: 80, duration_secs: 0.0 };
    assert!(atlas::motion::blocking(&atlas::motion::check(still, &spec)).is_empty(), "the source check can't see this");
    let plan = atlas::filmstrip::Plan::for_svg(still, &spec, 8);
    // Given a relative folder, as `atlas film ..\\still.svg` gives it: the
    // browser needs absolute paths, and this failed on the laptop until it did.
    let here = std::env::current_dir().unwrap();
    let rel = pathdiff(&dir, &here);
    let made = atlas::filmstrip::film(still, &plan, &browser, None, &rel, "still").unwrap();
    println!("LIVE [filmstrip, a fake animation] {}", made.say().replace('\n', " "));
    assert_eq!(made.distinct, 1);
    assert!(made.findings.iter().any(|f| f.rule == "moves when played"));
}

/// `to` written relative to `from`, with `..` as needed.
fn pathdiff(to: &Path, from: &Path) -> PathBuf {
    let (t, f): (Vec<_>, Vec<_>) = (to.components().collect(), from.components().collect());
    let common = t.iter().zip(&f).take_while(|(a, b)| a == b).count();
    let mut out = PathBuf::new();
    for _ in common..f.len() {
        out.push("..");
    }
    for c in &t[common..] {
        out.push(c.as_os_str());
    }
    out
}

fn scene_json() -> &'static str {
    r##"{"width":240,"height":160,
        "camera":{"from":[3.2,2.4,5.0],"at":[0,0.7,0],"fov":40},
        "sun":[-0.6,1.0,0.7],
        "objects":[{"shape":"ground","y":0,"colour":"#d8d2c4"},
                   {"shape":"box","at":[0,0.4,0],"size":[1.6,0.8,1.0],"colour":"#3a6fc4"},
                   {"shape":"sphere","at":[0,1.25,0],"radius":0.45,"colour":"#d9730d","shine":0.4},
                   {"shape":"cylinder","at":[1.4,0,0.6],"radius":0.25,"height":1.1,"colour":"#4f9a55"}]}"##
}

#[test]
fn a_3d_scene_is_drawn_in_house_as_a_still_and_a_turntable() {
    let dir = tmp("scene");
    let scene = atlas::scene3d::parse_scene(scene_json()).unwrap();
    let started = std::time::Instant::now();
    let made = atlas::scene3d::make(&scene, &dir, "desk", 12, None).unwrap();
    println!("LIVE [scene3d] {} ({} ms)", made.say().replace('\n', " "), started.elapsed().as_millis());
    assert!(atlas::motion::blocking(&made.findings).is_empty(), "{:?}", made.findings);
    let img = read_png(&std::fs::read(&made.still).unwrap()).unwrap();
    // The orange ball sits above the blue box; the green post is to the right.
    let p = |c: [u8; 4]| (c[0] as i32, c[1] as i32, c[2] as i32);
    let (ox, oy) = centre_of(&img, |c| { let (r, g, b) = p(c); r > g + 40 && g > b + 30 }).expect("orange ball");
    let (bx, by) = centre_of(&img, |c| { let (r, g, b) = p(c); b > r + 50 && b > g + 20 }).expect("blue box");
    let (gx, _) = centre_of(&img, |c| { let (r, g, b) = p(c); g > r + 30 && g > b + 30 }).expect("green post");
    println!("LIVE [scene3d] ball at ({ox:.0},{oy:.0}), box at ({bx:.0},{by:.0}), post at x={gx:.0}: ball above the box, post to its right");
    assert!(oy < by, "the ball should be drawn above the box");
    assert!(gx > bx, "the post should be to the right of the box");
    if have("ffmpeg") {
        let n = ffmpeg_frames(made.turntable.as_ref().unwrap(), 240, 160).len();
        println!("LIVE [scene3d] turntable GIF plays {n} frames in ffmpeg");
        assert_eq!(n, 12);
    }
    assert!(made.notes.iter().any(|n| n.contains("Blender isn't installed")) || made.blender.is_some());
}

#[test]
fn the_blender_script_for_a_scene_is_valid_python() {
    if !have("python3") {
        return println!("LIVE [blender script] skipped: no python3 here");
    }
    let dir = tmp("blend");
    let scene = atlas::scene3d::parse_scene(scene_json()).unwrap();
    let script = atlas::scene3d::blender_script(&scene, &dir.join("out.png"));
    let path = dir.join("scene.py");
    std::fs::write(&path, &script).unwrap();
    let st = std::process::Command::new("python3").args(["-m", "py_compile"]).arg(&path).status().unwrap();
    assert!(st.success(), "the Blender script doesn't compile:\n{script}");
    // One Blender object per shape, and the camera and sun.
    let adds = script.matches("bpy.ops.mesh.primitive_").count();
    assert_eq!(adds, 4);
    assert_eq!(script.matches("bpy.ops.object.light_add").count() + script.matches("bpy.ops.object.camera_add").count(), 2);
    println!("LIVE [blender script] {} lines, compiles, 4 shapes + sun + camera; runs when Blender is installed (not in this container)", script.lines().count());
}

struct Draws;
impl atlas::brain::Llm for Draws {
    fn complete(&self, system: &str, _user: &str) -> atlas::error::Result<String> {
        if system == atlas::scene3d::SCENE_SYSTEM {
            return Ok(format!("```json\n{}\n```", scene_json()));
        }
        if system == atlas::motion::MOTION_SYSTEM {
            return Ok(format!("```svg\n{BALL}\n```"));
        }
        Ok("{\"action\":\"say\",\"arg\":null,\"say\":\"?\"}".into())
    }
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn daemon_with<'a>(tag: &str, c: &'a Config, p: &'a MockPlatform) -> Daemon<'a> {
    Daemon::new(c, p, Some(std::sync::Arc::new(Draws)), Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

#[test]
fn asking_for_a_3d_scene_draws_it() {
    let c = Config::load(Path::new("config")).unwrap();
    let parser = atlas::intent::Parser::new(&c.commands);
    assert_eq!(
        parser.parse("draw a 3d scene of a ball on a box"),
        atlas::intent::Intent::Scene("scene of a ball on a box".into())
    );
    let p = plat();
    let mut d = daemon_with("scene-turn", &c, &p);
    let said = d.turn("draw a 3d scene of a ball on a box", 1_790_000_000);
    println!("LIVE [you: \"draw a 3d scene of a ball on a box\"]  Atlas: {}", said.replace('\n', " "));
    assert!(said.contains("Drew it"), "{said}");
    let png = said.split("Drew it: ").nth(1).and_then(|s| s.split(".png").next()).map(|s| format!("{s}.png")).unwrap();
    assert!(read_png(&std::fs::read(&png).unwrap()).is_ok());
}

#[test]
fn asking_for_an_animation_now_also_gives_a_gif() {
    let Some(browser) = a_browser() else { return println!("LIVE [animate] skipped: no browser here") };
    let mut c = Config::load(Path::new("config")).unwrap();
    let mut tools = c.tools.clone().unwrap_or_default();
    tools.vars.insert("browser".into(), browser.display().to_string());
    c.tools = Some(tools);
    let p = plat();
    let mut d = daemon_with("animate-turn", &c, &p);
    let said = d.turn("animate a ball rolling right, 200x100, for 2 seconds", 1_790_000_000);
    println!("LIVE [you: \"animate a ball rolling right, 200x100, for 2 seconds\"]  Atlas: {}", said.replace('\n', " "));
    assert!(said.contains("saved a GIF"), "{said}");
}
