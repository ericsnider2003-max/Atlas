//! Photo editing (`photo`, `straighten`, `cutout`), run with the real ffmpeg
//! on pictures made here, and the real cut-out models when they're on disk.
//!
//! ffmpeg: `ATLAS_TEST_FFMPEG` if set, else `ffmpeg` on the PATH; with
//! neither, the ffmpeg tests say they were skipped. The iPhone (HEIC) tests
//! need ffmpeg 8.1 or later to assemble the tiles; with an older one they
//! check Atlas says so instead of handing back one 512-pixel tile.
//!
//! The models and real photos: `ATLAS_PHOTO_KIT` names a folder holding
//! `models/modnet.onnx`, `models/u2netp.onnx` (what `atlas get photos`
//! fetches), `portrait.jpg` (a person), and optionally `iphone.heic` (a real
//! tiled iPhone photo). Without it those tests say they were skipped.

use atlas::photo::{self, Op, Outcome, Setup, Wish};
use std::path::{Path, PathBuf};

fn ffmpeg() -> Option<String> {
    if let Ok(p) = std::env::var("ATLAS_TEST_FFMPEG") {
        return Some(p);
    }
    atlas::tools::which("ffmpeg")
}

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-photo-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn setup(ff: &str, dir: &Path, models: Option<&Path>) -> Setup {
    Setup {
        ffmpeg: ff.to_string(),
        models: models.map(Path::to_path_buf).unwrap_or_else(|| dir.join("no-models")),
        state: dir.join("state"),
        scratch: dir.join("scratch"),
        install: dir.join("install"),
    }
}

/// Make a picture with ffmpeg's own generators.
fn make(ff: &str, source: &str, out: &Path) {
    let ok = std::process::Command::new(ff)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", source, "-frames:v", "1", "-update", "1"])
        .arg(out)
        .status()
        .unwrap();
    assert!(ok.success(), "couldn't make {source}");
}

/// Decode a picture the way any viewer would (upright), independently of
/// the code under test: width, height, RGB bytes.
fn pixels(ff: &str, path: &Path) -> (u32, u32, Vec<u8>) {
    pixels_with(ff, path, &[])
}

fn pixels_with(ff: &str, path: &Path, extra: &[&str]) -> (u32, u32, Vec<u8>) {
    let out = std::process::Command::new(ff)
        .args(["-v", "error"])
        .args(extra)
        .arg("-i")
        .arg(path)
        .args(["-frames:v", "1", "-f", "image2pipe", "-c:v", "ppm", "-pix_fmt", "rgb24", "-"])
        .output()
        .unwrap();
    let b = out.stdout;
    assert!(b.starts_with(b"P6"), "{}: {}", path.display(), String::from_utf8_lossy(&out.stderr));
    let head: Vec<&[u8]> = b.splitn(5, |c| c.is_ascii_whitespace()).collect();
    let w: u32 = std::str::from_utf8(head[1]).unwrap().parse().unwrap();
    let h: u32 = std::str::from_utf8(head[2]).unwrap().parse().unwrap();
    let data = head[4][..(w * h * 3) as usize].to_vec();
    (w, h, data)
}

fn mean_luma(rgb: &[u8]) -> f32 {
    let n = rgb.len() / 3;
    rgb.chunks_exact(3).map(|p| 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32).sum::<f32>() / n as f32 / 255.0
}

fn at(w: u32, rgb: &[u8], x: u32, y: u32) -> [u8; 3] {
    let i = ((y * w + x) * 3) as usize;
    [rgb[i], rgb[i + 1], rgb[i + 2]]
}

fn made(o: Outcome) -> (PathBuf, PathBuf, String) {
    match o {
        Outcome::Made { original, copy, said } => (original, copy, said),
        other => panic!("expected a copy, got {other:?}"),
    }
}

macro_rules! need_ffmpeg {
    () => {
        match ffmpeg() {
            Some(f) => f,
            None => {
                eprintln!("skipped: no ffmpeg (set ATLAS_TEST_FFMPEG)");
                return;
            }
        }
    };
}

// ------------------------------------------------------------------ words

#[test]
fn the_words_say_what_to_do() {
    let w = photo::read_wish("make this photo brighter");
    assert_eq!(w.ops, vec![Op::Lighter(true)]);
    assert!(!w.folder);
    let w = photo::read_wish("crop it for Instagram");
    assert!(matches!(&w.ops[..], [Op::Fit(p)] if p.name == "Instagram portrait"), "{w:?}");
    let w = photo::read_wish("crop it for an instagram story");
    assert!(matches!(&w.ops[..], [Op::Fit(p)] if p.width == 1080 && p.height == 1920), "{w:?}");
    let w = photo::read_wish("resize these for a YouTube thumbnail");
    assert!(w.folder);
    assert!(matches!(&w.ops[..], [Op::Fit(p)] if p.width == 3840 && p.height == 2160), "{w:?}");
    assert_eq!(photo::read_wish("straighten this").ops, vec![Op::Straighten]);
    assert_eq!(photo::read_wish("yes, straighten it").ops, vec![Op::StraightenAccepted]);
    assert_eq!(photo::read_wish("rotate it 2.5 degrees anticlockwise").ops, vec![Op::Tilt(-2.5)]);
    assert_eq!(photo::read_wish("rotate it left").ops, vec![Op::Turn(270)]);
    assert_eq!(photo::read_wish("turn it upside down").ops, vec![Op::Turn(180)]);
    assert_eq!(photo::read_wish("blur the background").ops, vec![Op::BlurBackground]);
    assert_eq!(photo::read_wish("remove the background").ops, vec![Op::RemoveBackground]);
    let w = photo::read_wish("fix all the photos in this folder");
    assert!(w.folder && w.ops == vec![Op::AutoFix], "{w:?}");
    let w = photo::read_wish("make it black and white and sharpen it, as a png");
    assert_eq!(w.ops, vec![Op::Grey, Op::Sharpen]);
    assert_eq!(w.format, Some(photo::Format::Png));
    assert!(photo::read_wish("undo the photo edit").undo);
    assert!(photo::read_wish("get the photo models").get_models);
    // Plain sizes, the shape kept -- and numbers that aren't sizes left alone.
    use photo::Size;
    assert_eq!(photo::read_wish("resize it to 1920 wide").ops, vec![Op::Resize(Size::Wide(1920))]);
    assert_eq!(photo::read_wish("make it 2000 pixels tall").ops, vec![Op::Resize(Size::Tall(2000))]);
    assert_eq!(photo::read_wish("resize these to 1080x1080").ops, vec![Op::Resize(Size::Within(1080, 1080))]);
    assert_eq!(photo::read_wish("make it half size").ops, vec![Op::Resize(Size::Scale(0.5))]);
    assert_eq!(photo::read_wish("shrink it to 25%").ops, vec![Op::Resize(Size::Scale(0.25))]);
    assert_eq!(photo::read_wish("rotate it 90 degrees").ops, vec![Op::Turn(90)]);
    assert!(photo::read_wish("fix the 3 photos from saturday").ops == vec![Op::AutoFix]);
    assert_eq!(Size::Wide(800).of(1600, 1200), (800, 600));
    assert_eq!(Size::Within(1000, 1000).of(1600, 1200), (1000, 750));
    // Sharpening comes after the resize, never before.
    let w = photo::read_wish("sharpen it and crop it for instagram");
    assert!(matches!(&w.ops[..], [Op::Fit(_), Op::Sharpen]), "{w:?}");
}

#[test]
fn the_voice_and_the_hub_reach_photo_editing() {
    let p = atlas::intent::Parser::new(&atlas::config::Config::load(Path::new("config")).unwrap().commands);
    for said in [
        "make this photo brighter",
        "crop it for Instagram",
        "straighten this",
        "blur the background",
        "remove the background",
        "resize these for a YouTube thumbnail",
        "fix all the photos in this folder",
        "yes, straighten it",
        "undo the photo edit",
        "do the same to the whole folder",
        "resize this photo to 1920 wide",
        // What the Documents page's form sends.
        "edit this photo \"C:\\Users\\Maya\\Pictures\\trip.jpg\" crop it for instagram",
    ] {
        assert!(matches!(p.parse(said), atlas::intent::Intent::EditPhoto(_)), "{said:?} -> {:?}", p.parse(said));
    }
    // A bare "undo" is still Atlas's general undo, not a photo's.
    assert!(!matches!(p.parse("undo"), atlas::intent::Intent::EditPhoto(_)));
}

#[test]
fn the_documents_page_offers_to_edit_a_photo_and_nothing_else() {
    let row = |name: &str, photo: Option<&str>| atlas::hubpages::DocRow {
        id: 7,
        name: name.into(),
        kind: "Photo".into(),
        area: "Personal".into(),
        shared: String::new(),
        private: true,
        when: "today".into(),
        state: "Read".into(),
        photo: photo.map(String::from),
    };
    let page = atlas::hubpages::documents_page(&[row("trip.jpg", Some("C:\\pics\\trip's.jpg")), row("notes.pdf", None)], &[], None);
    assert_eq!(page.matches("action='/hub/talk'").count(), 1, "one photo, one form");
    // The path is escaped, and the sentence is the one the voice path reads.
    assert!(page.contains("edit this photo &quot;C:\\pics\\trip&#39;s.jpg&quot; crop it for instagram"), "{page}");
    for (said, _) in atlas::hubpages::PHOTO_EDITS {
        let w = photo::read_wish(said);
        assert!(!w.ops.is_empty() || w.undo, "the page offers {said:?}, which does nothing");
    }
}

#[test]
fn one_table_of_sizes_and_the_checked_ones_are_the_platforms_own() {
    let mut names = std::collections::HashSet::new();
    for p in photo::PRESETS {
        assert!(names.insert(p.name), "{} twice", p.name);
        assert!(p.width >= 566 && p.height >= 566, "{}", p.name);
    }
    let by = |n: &str| photo::PRESETS.iter().find(|p| p.name == n).unwrap();
    // YouTube help 72431, read 29 Sep 2026.
    assert_eq!((by("YouTube thumbnail").width, by("YouTube thumbnail").height), (3840, 2160));
    assert_eq!((by("YouTube Shorts thumbnail").width, by("YouTube Shorts thumbnail").height), (2160, 3840));
    assert_eq!(by("YouTube thumbnail").max_bytes, Some(2_000_000));
    // Buffer, Mar 2026.
    assert_eq!((by("Instagram portrait").width, by("Instagram portrait").height), (1080, 1350));
    assert_eq!((by("Instagram landscape").width, by("Instagram landscape").height), (1080, 566));
    assert!(!by("TikTok photo").checked && !by("X post").checked, "secondary-guide sizes must say so");
}

#[test]
fn a_copy_is_named_beside_the_original_and_never_over_anything() {
    let dir = tmp("names");
    let orig = dir.join("trip.jpg");
    std::fs::write(&orig, b"x").unwrap();
    assert_eq!(photo::copy_path(&orig, photo::Format::Jpeg), dir.join("trip.edited.jpg"));
    std::fs::write(dir.join("trip.edited.jpg"), b"y").unwrap();
    assert_eq!(photo::copy_path(&orig, photo::Format::Jpeg), dir.join("trip.edited-2.jpg"));
    // Editing a copy numbers on rather than piling up ".edited.edited".
    assert_eq!(photo::copy_path(&dir.join("trip.edited.jpg"), photo::Format::Png), dir.join("trip.edited.png"));
    assert_eq!(photo::copy_path(&dir.join("trip.edited.jpg"), photo::Format::Jpeg), dir.join("trip.edited-2.jpg"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn ffmpeg_versions_are_read_from_every_kind_of_build() {
    assert_eq!(photo::ffmpeg_version("ffmpeg version 9.0.2-essentials_build-www.gyan.dev Copyright"), Some((9, 0)));
    assert_eq!(photo::ffmpeg_version("ffmpeg version 7.1.1-essentials_build-www.gyan.dev"), Some((7, 1)));
    assert_eq!(photo::ffmpeg_version("ffmpeg version n9.0.2-14-gebafaee10a-20260928 Copyright"), Some((9, 0)));
    assert_eq!(photo::ffmpeg_version("ffmpeg version 6.1.1-3ubuntu5 Copyright"), Some((6, 1)));
    assert!(photo::ffmpeg_version("ffmpeg version N-120000-gabc Copyright").unwrap() >= photo::HEIC_FROM);
}

// ------------------------------------------------------------------ the tilt

/// Stripes leaning `lean` degrees anticlockwise, as grey values, smoothed
/// the way a camera's are (each pixel the average of 4x4 samples).
fn stripes(w: usize, h: usize, lean: f32) -> Vec<f32> {
    let (s, c) = lean.to_radians().sin_cos();
    let ink = |x: f32, y: f32| if (y * c + x * s).rem_euclid(40.0) < 5.0 { 20.0 } else { 230.0 };
    (0..h)
        .flat_map(|y| {
            (0..w).map(move |x| {
                let mut v = 0.0;
                for i in 0..4 {
                    for j in 0..4 {
                        v += ink(x as f32 + (i as f32 + 0.5) / 4.0, y as f32 + (j as f32 + 0.5) / 4.0);
                    }
                }
                v / 16.0
            })
        })
        .collect()
}

#[test]
fn the_tilt_of_straight_lines_is_measured_to_a_fraction_of_a_degree() {
    for lean in [-6.0f32, -2.3, -0.8, 1.5, 4.0, 8.5] {
        let t = atlas::straighten::measure(&stripes(512, 384, lean), 512, 384).unwrap();
        assert!(t.sure(), "{lean}: {t:?}");
        assert!((t.fix - lean).abs() < 0.25, "leaning {lean}° anticlockwise, measured a fix of {}", t.fix);
    }
    let level = atlas::straighten::measure(&stripes(512, 384, 0.0), 512, 384).unwrap();
    assert!(level.level(), "{level:?}");
    // Noise has no direction: not offered.
    let mut seed = 7u32;
    let noise: Vec<f32> = (0..512 * 384)
        .map(|_| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            ((seed >> 16) & 255) as f32
        })
        .collect();
    assert!(atlas::straighten::measure(&noise, 512, 384).is_none_or(|t| !t.sure()), "noise was offered a straightening");
    // The crop after turning keeps the most that has no empty corner.
    let k = atlas::straighten::crop_after_turning(4000.0, 3000.0, 3.0);
    assert!((0.90..0.95).contains(&k), "{k}");
    assert_eq!(atlas::straighten::crop_after_turning(4000.0, 3000.0, 0.0), 1.0);
}

#[test]
fn straightening_is_offered_then_done_only_on_yes() {
    let ff = need_ffmpeg!();
    let dir = tmp("straighten");
    let photo_path = dir.join("shelf.png");
    // Level lines, then turned 3° anticlockwise by ffmpeg itself (a negative
    // angle is anticlockwise in its `rotate`).
    make(&ff, "color=white:s=900x700,drawgrid=w=900:h=50:t=5:c=black,rotate=-3*PI/180:c=white,crop=700:500", &photo_path);
    let before = std::fs::read(&photo_path).unwrap();
    let su = setup(&ff, &dir, None);
    let st = std::fs::create_dir_all(&su.state);
    assert!(st.is_ok());

    // Asked: measured and offered, nothing written.
    let plan = photo::ask(&format!("straighten this \"{}\"", photo_path.display()), None, su.clone());
    let photo::Plan::Later { work, .. } = plan else { panic!("straightening should run as work") };
    let said = work(&|| false);
    assert!(said.contains("3.0°") || said.contains("2.9°") || said.contains("3.1°"), "{said}");
    assert!(said.contains("Straighten it?"), "{said}");
    assert!(!dir.join("shelf.edited.png").exists(), "straightening was applied without a yes");

    // Yes: done, on a copy, and the copy measures level.
    let photo::Plan::Later { work, .. } = photo::ask("yes, straighten it", None, su.clone()) else { panic!() };
    let said = work(&|| false);
    let copy = dir.join("shelf.edited.png");
    assert!(copy.is_file(), "{said}");
    assert!(said.contains("clockwise"), "{said}");
    let look = photo::look_at_photo(&ff, &copy).unwrap();
    let t = atlas::straighten::measure(&atlas::straighten::grey(&look.small.rgb), look.small.width as usize, look.small.height as usize).unwrap();
    assert!(t.fix.abs() < 0.4, "still {}° off after straightening", t.fix);
    assert_eq!(std::fs::read(&photo_path).unwrap(), before, "the original changed");
    let _ = std::fs::remove_dir_all(dir);
}

// ------------------------------------------------------------------ edits

#[test]
fn brighter_is_measurably_brighter_and_the_original_is_untouched() {
    let ff = need_ffmpeg!();
    let dir = tmp("brighter");
    let p = dir.join("dim.jpg");
    make(&ff, "testsrc2=s=800x600,eq=brightness=-0.3:contrast=0.7", &p);
    let before = std::fs::read(&p).unwrap();
    let su = setup(&ff, &dir, None);
    let (_, copy, said) = made(photo::edit_one(&su, &p, &photo::read_wish("make this photo brighter"), None, false));
    assert_eq!(copy, dir.join("dim.edited.jpg"));
    assert!(said.contains("average brightness"), "{said}");
    let (_, _, a) = pixels(&ff, &p);
    let (w, h, b) = pixels(&ff, &copy);
    assert_eq!((w, h), (800, 600));
    assert!(mean_luma(&b) > mean_luma(&a) + 0.05, "{} -> {}", mean_luma(&a), mean_luma(&b));
    assert_eq!(std::fs::read(&p).unwrap(), before, "the original changed");

    // Again: a second copy beside the first, the first left alone.
    let first = std::fs::read(&copy).unwrap();
    let (_, copy2, _) = made(photo::edit_one(&su, &p, &photo::read_wish("darker"), None, false));
    assert_eq!(copy2, dir.join("dim.edited-2.jpg"));
    assert_eq!(std::fs::read(&copy).unwrap(), first);
    let (_, _, c) = pixels(&ff, &copy2);
    assert!(mean_luma(&c) < mean_luma(&a) - 0.03);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_colour_cast_and_flat_light_are_fixed_by_the_numbers() {
    let ff = need_ffmpeg!();
    let dir = tmp("autofix");
    let p = dir.join("blue.png");
    // Flat, dark and blue.
    make(&ff, "testsrc2=s=640x480,eq=contrast=0.5:brightness=-0.15,colorchannelmixer=rr=0.8:bb=1.2", &p);
    let su = setup(&ff, &dir, None);
    let (_, copy, said) = made(photo::edit_one(&su, &p, &photo::read_wish("fix the colours and exposure"), None, false));
    assert!(said.contains("blue cast"), "{said}");
    let spread = |rgb: &[u8]| {
        let s = photo::stats(&photo::Picture { width: 0, height: 0, rgb: rgb.to_vec() });
        (s.channel[2] - s.channel[1]).abs() + (s.channel[0] - s.channel[1]).abs()
    };
    let (_, _, a) = pixels(&ff, &p);
    let (_, _, b) = pixels(&ff, &copy);
    assert!(spread(&b) < spread(&a) * 0.6, "cast {} -> {}", spread(&a), spread(&b));
    let (sa, sb) = (
        photo::stats(&photo::Picture { width: 0, height: 0, rgb: a }),
        photo::stats(&photo::Picture { width: 0, height: 0, rgb: b }),
    );
    assert!(sb.p99 - sb.p1 > (sa.p99 - sa.p1) + 0.1, "contrast wasn't stretched: {sa:?} -> {sb:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn every_platform_size_comes_out_exactly_that_size() {
    let ff = need_ffmpeg!();
    let dir = tmp("presets");
    let p = dir.join("wide.jpg");
    make(&ff, "testsrc2=s=1600x1200", &p);
    let su = setup(&ff, &dir, None);
    for preset in photo::PRESETS {
        let w = Wish { ops: vec![Op::Fit(*preset)], ..Wish::default() };
        let (_, copy, said) = made(photo::edit_one(&su, &p, &w, None, false));
        let (cw, ch, _) = pixels(&ff, &copy);
        assert_eq!((cw, ch), (preset.width, preset.height), "{}: {said}", preset.name);
        if let Some(max) = preset.max_bytes {
            assert!(std::fs::metadata(&copy).unwrap().len() <= max, "{} is over {max} bytes", preset.name);
        }
        if preset.height > 1200 {
            assert!(said.contains("enlarged"), "{}: an enlarged photo wasn't said: {said}", preset.name);
        }
        std::fs::remove_file(copy).unwrap();
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_plain_resize_keeps_the_shape() {
    let ff = need_ffmpeg!();
    let dir = tmp("resize");
    let p = dir.join("big.jpg");
    make(&ff, "testsrc2=s=1600x1200", &p);
    let su = setup(&ff, &dir, None);
    for (said, want) in [("resize it to 800 wide", (800, 600)), ("make it half size", (800, 600)), ("resize it to 1000x1000", (1000, 750))] {
        let (_, copy, words) = made(photo::edit_one(&su, &p, &photo::read_wish(said), None, false));
        let (w, h, _) = pixels(&ff, &copy);
        assert_eq!((w, h), want, "{said}: {words}");
        assert!(words.contains(&format!("{}x{}", want.0, want.1)), "{words}");
        std::fs::remove_file(copy).unwrap();
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn do_the_same_to_the_whole_folder_repeats_the_last_edit() {
    let ff = need_ffmpeg!();
    let dir = tmp("same");
    let pics = dir.join("Trip");
    std::fs::create_dir_all(&pics).unwrap();
    for (i, src) in ["testsrc2=s=320x240", "smptebars=s=320x240"].iter().enumerate() {
        make(&ff, &format!("{src},eq=brightness=-0.3:contrast=0.7"), &pics.join(format!("p{i}.jpg")));
    }
    let su = setup(&ff, &dir, None);
    // Nothing done yet: nothing to repeat, said so.
    match photo::ask("do the same to the whole folder", None, su.clone()) {
        photo::Plan::Now(s) => assert!(s.contains("nothing to do the same as"), "{s}"),
        _ => panic!("repeated an edit that never happened"),
    }
    let first = pics.join("p0.jpg");
    let photo::Plan::Later { work, .. } = photo::ask("make this photo brighter and resize it to 160 wide", Some(first.display().to_string()), su.clone()) else { panic!() };
    work(&|| false);
    assert!(pics.join("p0.edited.jpg").is_file());
    // The same, to the folder of the photo last handed over.
    let photo::Plan::Later { start, work } = photo::ask("do the same to the whole folder", Some(first.display().to_string()), su.clone()) else { panic!() };
    assert!(start.contains("2 photos"), "{start}");
    let said = work(&|| false);
    assert!(said.contains("Done 2 of the 2"), "{said}");
    let copy = pics.join("p1.edited.jpg");
    let (w, h, b) = pixels(&ff, &copy);
    let (_, _, a) = pixels(&ff, &pics.join("p1.jpg"));
    assert_eq!((w, h), (160, 120), "the resize wasn't repeated");
    assert!(mean_luma(&b) > mean_luma(&a) + 0.05, "the brightening wasn't repeated");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn turning_and_flipping_move_the_pixels_the_right_way() {
    let ff = need_ffmpeg!();
    let dir = tmp("turn");
    let p = dir.join("flag.png");
    // Red, with a blue band down the left side.
    make(&ff, "color=red:s=64x32,drawbox=x=0:y=0:w=16:h=32:color=blue:t=fill", &p);
    let su = setup(&ff, &dir, None);
    let (_, copy, _) = made(photo::edit_one(&su, &p, &photo::read_wish("rotate it right"), None, false));
    let (w, h, px) = pixels(&ff, &copy);
    assert_eq!((w, h), (32, 64));
    // A quarter turn clockwise puts the left side on top.
    assert!(at(w, &px, 16, 4)[2] > 200 && at(w, &px, 16, 60)[0] > 200, "{:?} {:?}", at(w, &px, 16, 4), at(w, &px, 16, 60));
    let (_, copy, _) = made(photo::edit_one(&su, &p, &photo::read_wish("flip it"), None, false));
    let (w, _, px) = pixels(&ff, &copy);
    assert!(at(w, &px, 60, 16)[2] > 200 && at(w, &px, 4, 16)[0] > 200);
    let _ = std::fs::remove_dir_all(dir);
}

/// A JPEG whose EXIF says "turn 90° clockwise to show" (orientation 6).
fn exif_turned(ff: &str, dir: &Path) -> PathBuf {
    let base = dir.join("base.jpg");
    make(ff, "color=red:s=64x32,drawbox=x=0:y=0:w=16:h=32:color=blue:t=fill", &base);
    let d = std::fs::read(&base).unwrap();
    let mut tiff = b"II*\0".to_vec();
    tiff.extend(8u32.to_le_bytes());
    tiff.extend(1u16.to_le_bytes());
    tiff.extend(0x0112u16.to_le_bytes());
    tiff.extend(3u16.to_le_bytes());
    tiff.extend(1u32.to_le_bytes());
    tiff.extend(6u16.to_le_bytes());
    tiff.extend(0u16.to_le_bytes());
    tiff.extend(0u32.to_le_bytes());
    let mut app1 = b"Exif\0\0".to_vec();
    app1.extend(tiff);
    let mut out = d[..2].to_vec();
    out.extend([0xFF, 0xE1]);
    out.extend(((app1.len() + 2) as u16).to_be_bytes());
    out.extend(app1);
    out.extend(&d[2..]);
    let p = dir.join("sideways.jpg");
    std::fs::write(&p, out).unwrap();
    p
}

#[test]
fn a_photo_tagged_to_be_turned_comes_out_upright_and_untagged() {
    let ff = need_ffmpeg!();
    let dir = tmp("exif");
    let p = exif_turned(&ff, &dir);
    // As stored it's 64x32; shown, it's 32x64.
    assert_eq!(pixels_with(&ff, &p, &["-noautorotate"]).0, 64);
    let su = setup(&ff, &dir, None);
    let (_, copy, _) = made(photo::edit_one(&su, &p, &photo::read_wish("brighter"), None, false));
    let (w, h, px) = pixels(&ff, &copy);
    assert_eq!((w, h), (32, 64), "the copy isn't upright");
    assert!(at(w, &px, 16, 4)[2] > 150, "the blue band should be on top: {:?}", at(w, &px, 16, 4));
    // No tag left to turn it a second time.
    let (w2, h2, _) = pixels_with(&ff, &copy, &["-noautorotate"]);
    assert_eq!((w2, h2), (32, 64), "the copy still carries an orientation tag");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_folder_is_done_photo_by_photo_and_undo_takes_it_all_back() {
    let ff = need_ffmpeg!();
    let dir = tmp("folder");
    let pics = dir.join("Holiday");
    std::fs::create_dir_all(&pics).unwrap();
    for (i, src) in ["testsrc2=s=320x240", "smptebars=s=320x240", "rgbtestsrc=s=320x240"].iter().enumerate() {
        make(&ff, src, &pics.join(format!("p{i}.jpg")));
    }
    // An edited copy already there is not edited again.
    std::fs::copy(pics.join("p0.jpg"), pics.join("p0.edited.jpg")).unwrap();
    std::fs::write(pics.join("notes.txt"), "not a photo").unwrap();
    let before: Vec<Vec<u8>> = (0..3).map(|i| std::fs::read(pics.join(format!("p{i}.jpg"))).unwrap()).collect();
    let su = setup(&ff, &dir, None);

    let photo::Plan::Later { start, work } = photo::ask(&format!("fix all the photos in \"{}\"", pics.display()), None, su.clone()) else { panic!() };
    assert!(start.contains("3 photos"), "{start}");
    let said = work(&|| false);
    assert!(said.contains("Done 3 of the 3"), "{said}");
    for i in 0..3 {
        assert_eq!(std::fs::read(pics.join(format!("p{i}.jpg"))).unwrap(), before[i]);
    }
    assert!(pics.join("p0.edited-2.jpg").is_file() && pics.join("p1.edited.jpg").is_file() && pics.join("p2.edited.jpg").is_file());
    assert!(!pics.join("p0.edited.edited.jpg").exists());

    let undone = photo::undo_photo_edit(&su.state);
    assert!(undone.contains("Deleted the 3 edited copies"), "{undone}");
    assert!(!pics.join("p1.edited.jpg").exists() && !pics.join("p0.edited-2.jpg").exists());
    // The copy that was there before is not the last edit's, so it stays.
    assert!(pics.join("p0.edited.jpg").is_file());
    for i in 0..3 {
        assert_eq!(std::fs::read(pics.join(format!("p{i}.jpg"))).unwrap(), before[i]);
    }
    assert!(photo::undo_photo_edit(&su.state).contains("no photo edit"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn now_crop_it_carries_on_from_the_last_copy_and_undo_says_where_the_original_is() {
    let ff = need_ffmpeg!();
    let dir = tmp("chain");
    let p = dir.join("me.jpg");
    make(&ff, "testsrc2=s=1600x1200", &p);
    let su = setup(&ff, &dir, None);
    let handed = Some(p.display().to_string());
    let photo::Plan::Later { work, .. } = photo::ask("make this photo brighter", handed.clone(), su.clone()) else { panic!() };
    work(&|| false);
    let photo::Plan::Later { work, .. } = photo::ask("crop it for instagram", handed, su.clone()) else { panic!() };
    let said = work(&|| false);
    assert!(said.contains("me.edited-2.jpg"), "{said}");
    let (w, h, _) = pixels(&ff, &dir.join("me.edited-2.jpg"));
    assert_eq!((w, h), (1080, 1350));
    let undone = photo::undo_photo_edit(&su.state);
    assert!(undone.contains("me.edited-2.jpg") && undone.contains("me.edited.jpg"), "{undone}");
    assert!(!dir.join("me.edited-2.jpg").exists() && dir.join("me.edited.jpg").exists() && p.exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn no_photo_named_and_none_handed_over_is_a_question() {
    let dir = tmp("which");
    let su = setup("ffmpeg", &dir, None);
    match photo::ask("make this photo brighter", None, su.clone()) {
        photo::Plan::Now(s) => assert!(s.contains("Which photo") || s.contains("ffmpeg"), "{s}"),
        _ => panic!("nothing to work on, but work was started"),
    }
    match photo::ask("edit this photo", Some("x.jpg".into()), su) {
        photo::Plan::Now(s) => assert!(s.contains("What should I do"), "{s}"),
        _ => panic!(),
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// Through the daemon, as voice or the hub reach it: the branch
/// (`Intent::EditPhoto` -> `edit_photo` -> the crew) and not only `photo::ask`.
#[test]
fn saying_it_to_atlas_makes_a_brighter_copy_and_undo_takes_it_back() {
    let ff = need_ffmpeg!();
    let dir = tmp("daemon");
    let p = dir.join("dim.jpg");
    make(&ff, "testsrc2=s=640x480,eq=brightness=-0.3:contrast=0.7", &p);
    let before = std::fs::read(&p).unwrap();
    let mut c = atlas::config::Config::load(Path::new("config")).unwrap();
    c.tools.as_mut().unwrap().video.ffmpeg.command = ff.clone();
    let c: &'static atlas::config::Config = Box::leak(Box::new(c));
    let plat: &'static atlas::platform::mock::MockPlatform = Box::leak(Box::new(atlas::platform::mock::MockPlatform::new(vec![
        atlas::platform::Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true },
    ])));
    let mut d = atlas::daemon::Daemon::new(
        c,
        plat,
        None,
        atlas::store::Store::new(dir.join("store")),
        atlas::proactive::Proactive::new(atlas::proactive::ProactiveConfig::default()),
    );
    let when_done = |d: &mut atlas::daemon::Daemon<'static>, started: String| {
        let mut said = d.errands_done_for_test();
        if said.is_empty() { started } else { said.remove(0) }
    };
    let said = d.execute(&atlas::intent::Intent::EditPhoto(format!("make this photo brighter \"{}\"", p.display())));
    let said = when_done(&mut d, said);
    let copy = dir.join("dim.edited.jpg");
    assert!(copy.is_file() && said.contains("dim.edited.jpg"), "{said}");
    let (_, _, a) = pixels(&ff, &p);
    let (_, _, b) = pixels(&ff, &copy);
    assert!(mean_luma(&b) > mean_luma(&a) + 0.05, "{said}");
    assert_eq!(std::fs::read(&p).unwrap(), before, "the original changed");

    let said = d.execute(&atlas::intent::Intent::EditPhoto("undo the photo edit".into()));
    let said = when_done(&mut d, said);
    assert!(!copy.exists() && said.contains("dim.edited.jpg"), "{said}");
    assert_eq!(std::fs::read(&p).unwrap(), before, "undo touched the original");
    // The daemon writes its store as it's dropped: dropped first, so the
    // folder is gone for good.
    drop(d);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn this_photo_is_the_named_one_then_the_copied_or_handed_one() {
    let dir = tmp("this");
    let (named, copied, handed) = (dir.join("named.jpg"), dir.join("copied one.png"), dir.join("handed.heic"));
    for p in [&named, &copied, &handed] {
        std::fs::write(p, b"x").unwrap();
    }
    let clip = format!("\"{}\"", copied.display()); // Explorer's "Copy as path"
    let h = || Some(handed.display().to_string());
    let s = |p: &Path| Some(p.display().to_string());
    // A path in the words beats everything.
    assert_eq!(photo::which_photo(&format!("brighten \"{}\"", named.display()), Some(&clip), h()), s(&named));
    // "The photo I copied" means the clipboard, even with one handed over.
    assert_eq!(photo::which_photo("brighten the photo I copied", Some(&clip), h()), s(&copied));
    // Plain "this": the one handed over first, else what was copied.
    assert_eq!(photo::which_photo("brighten this photo", Some(&clip), h()), s(&handed));
    assert_eq!(photo::which_photo("brighten this photo", Some(&clip), None), s(&copied));
    // Two apostrophes are not a quoted path.
    assert_eq!(photo::which_photo("brighten Maya's photo, it's dark", None, h()), s(&handed));
    assert_eq!(photo::which_photo("brighten Maya's photo, it's dark", None, Some("s photo, it".into())), None);
    // Copied text that isn't a photo on this machine is not one.
    assert_eq!(photo::which_photo("brighten what I copied", Some("some words I copied"), None), None);
    assert_eq!(photo::which_photo("brighten what I copied", Some(&format!("{}\n{}", named.display(), copied.display())), None), None);
    assert_eq!(photo::which_photo("brighten what I copied", Some(&dir.join("gone.jpg").display().to_string()), None), None);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_missing_cut_out_model_is_a_sentence_and_no_file() {
    let ff = need_ffmpeg!();
    let dir = tmp("nomodel");
    let p = dir.join("me.jpg");
    make(&ff, "testsrc2=s=320x240", &p);
    let su = setup(&ff, &dir, None);
    match photo::edit_one(&su, &p, &photo::read_wish("blur the background"), None, false) {
        Outcome::Said(s) => assert!(s.contains("get the photo models") && s.contains("MB"), "{s}"),
        other => panic!("{other:?}"),
    }
    assert!(!dir.join("me.edited.jpg").exists());
    let _ = std::fs::remove_dir_all(dir);
}

// ------------------------------------------------------------------ iPhone photos

fn ffmpeg_reads_tiles(ff: &str) -> bool {
    let out = std::process::Command::new(ff).arg("-version").output().unwrap();
    photo::ffmpeg_version(&String::from_utf8_lossy(&out.stdout)).is_some_and(|v| v >= photo::HEIC_FROM)
}

#[test]
fn an_iphone_style_tiled_heic_comes_in_whole_and_the_right_way_up() {
    let ff = need_ffmpeg!();
    let dir = tmp("heic");
    let su = setup(&ff, &dir, None);
    for (name, size) in [("grid.heic", (1000, 900)), ("grid_irot1.heic", (900, 1000))] {
        let p = dir.join(name);
        std::fs::copy(Path::new("tests/fixtures/photo").join(name), &p).unwrap();
        let out = photo::edit_one(&su, &p, &photo::read_wish("as a png"), None, false);
        if !ffmpeg_reads_tiles(&ff) {
            match out {
                Outcome::Said(s) => assert!(s.contains("can't read one whole"), "{s}"),
                other => panic!("an old ffmpeg's one tile was taken for the photo: {other:?}"),
            }
            eprintln!("{name}: this ffmpeg is older than 8.1, so only the refusal was checked");
            continue;
        }
        let (_, copy, _) = made(out);
        let (w, h, px) = pixels(&ff, &copy);
        assert_eq!((w, h), size, "{name}: not the whole grid");
        // Red, green / blue, white, in that order -- turned a quarter
        // anticlockwise for the irot one.
        let corners = [at(w, &px, 50, 50), at(w, &px, w - 50, 50), at(w, &px, 50, h - 50), at(w, &px, w - 50, h - 50)];
        let want: [[bool; 3]; 4] = if size.0 == 1000 {
            [[true, false, false], [false, true, false], [false, false, true], [true, true, true]]
        } else {
            [[false, true, false], [true, true, true], [true, false, false], [false, false, true]]
        };
        for (c, w) in corners.iter().zip(want) {
            let lit = [c[0] > 100, c[1] > 90, c[2] > 100];
            assert_eq!(lit, w, "{name}: corners {corners:?}");
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}

// ------------------------------------------------------------------ the real models

fn kit() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var("ATLAS_PHOTO_KIT").ok()?);
    p.join("models").is_dir().then_some(p)
}

#[test]
fn each_cut_out_model_loads_in_tract_and_finds_the_person() {
    let (Some(kit), Some(ff)) = (kit(), ffmpeg()) else {
        eprintln!("skipped: ATLAS_PHOTO_KIT or ffmpeg isn't there");
        return;
    };
    let photo_path = kit.join("portrait.jpg");
    let seen = photo::look_at_photo(&ff, &photo_path).unwrap();
    for by in [atlas::cutout::Matter::Portrait, atlas::cutout::Matter::Anything] {
        let model = kit.join("models").join(by.file());
        if !model.is_file() {
            eprintln!("skipped {}: not in the kit", by.file());
            continue;
        }
        let (mw, mh) = by.input_size(seen.width, seen.height);
        let out = std::process::Command::new(&ff)
            .args(["-v", "error", "-i"])
            .arg(&photo_path)
            .args(["-vf", &format!("scale={mw}:{mh}:flags=area,format=rgb24"), "-frames:v", "1", "-f", "rawvideo", "-"])
            .output()
            .unwrap();
        let began = std::time::Instant::now();
        let m = atlas::cutout::matte(by, &model, &out.stdout, mw, mh).unwrap();
        eprintln!(
            "{}: {}x{} photo, model input {mw}x{mh}, load+run {} ms (run alone {} ms), subject {:.0}%",
            by.file(),
            seen.width,
            seen.height,
            began.elapsed().as_millis(),
            m.ms,
            m.subject_share() * 100.0
        );
        assert_eq!((m.width, m.height), (mw, mh));
        let share = m.subject_share();
        assert!((0.05..0.9).contains(&share), "{}: subject share {share}", by.file());
        // The person is in the middle; the top corners are background.
        let a = |x: u32, y: u32| m.alpha[(y * mw + x) as usize] as u32;
        let middle = a(mw / 2, mh / 2);
        let corners = (a(2, 2) + a(mw - 3, 2)) / 2;
        assert!(middle > 150 && corners < 60, "{}: middle {middle}, corners {corners}", by.file());
    }
}

#[test]
fn the_background_is_blurred_and_removed_for_real() {
    let (Some(kit), Some(ff)) = (kit(), ffmpeg()) else {
        eprintln!("skipped: ATLAS_PHOTO_KIT or ffmpeg isn't there");
        return;
    };
    let dir = tmp("background");
    let p = dir.join("portrait.jpg");
    std::fs::copy(kit.join("portrait.jpg"), &p).unwrap();
    let su = setup(&ff, &dir, Some(&kit.join("models")));
    let (ow, oh, orig) = pixels(&ff, &p);

    let t = std::time::Instant::now();
    let (_, cut, said) = made(photo::edit_one(&su, &p, &photo::read_wish("remove the background"), None, false));
    eprintln!("remove the background: {} ms -- {said}", t.elapsed().as_millis());
    assert_eq!(cut.extension().unwrap(), "png", "a see-through photo must not be a JPEG");
    // The alpha itself: see-through at the top corners, solid in the middle.
    let out = std::process::Command::new(&ff)
        .args(["-v", "error", "-i"])
        .arg(&cut)
        .args(["-vf", "alphaextract", "-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "gray", "-"])
        .output()
        .unwrap();
    let alpha = out.stdout;
    assert_eq!(alpha.len(), (ow * oh) as usize, "the cut-out isn't the photo's size");
    let a = |x: u32, y: u32| alpha[(y * ow + x) as usize];
    assert!(a(ow / 2, oh / 2) > 200 && a(5, 5) < 40 && a(ow - 6, 5) < 40, "alpha middle {} corners {} {}", a(ow / 2, oh / 2), a(5, 5), a(ow - 6, 5));
    // Where it's solid, the subject's own pixels are kept as they were.
    let (_, _, cut_rgb) = pixels(&ff, &cut);
    let (mut diff, mut n) = (0f64, 0u64);
    for i in 0..(ow * oh) as usize {
        if alpha[i] > 235 {
            for c in 0..3 {
                diff += (cut_rgb[i * 3 + c] as f64 - orig[i * 3 + c] as f64).abs();
            }
            n += 3;
        }
    }
    let kept = diff / n.max(1) as f64;
    eprintln!("subject pixels in the cut-out differ from the original by {kept:.2} on average");
    assert!(n > 3 * (ow * oh) as u64 / 20 && kept < 3.0, "the subject wasn't kept as it was ({kept:.2} over {n} values)");

    let t = std::time::Instant::now();
    // As a PNG: a JPEG's own re-encoding noise is about as much fine detail
    // as this concrete wall has (1.23 -> 0.68 as JPEG, 29 Sep 2026), which
    // would measure the encoder, not the blur. JPEG output is checked above.
    let (_, blurred, said) = made(photo::edit_one(&su, &p, &photo::read_wish("blur the background as a png"), None, false));
    eprintln!("blur the background: {} ms -- {said}", t.elapsed().as_millis());
    let (_, _, b) = pixels(&ff, &blurred);
    // 29 Sep 2026: this was measured at a 40-pixel corner, which was sky:
    // it had almost no detail to lose (0.37), so a copy with nothing blurred
    // passed too. Now split by the model's own matte from the cut-out above.
    // Background: fine detail must fall by half, and the pixels must move. Subject: detail and pixels kept.
    // Fine detail as the second difference across (|2p - left - right|):
    // a smooth brightness slope over the wall scores nothing, which the first
    // difference counted as detail and a blur keeps.
    let second = |px: &[u8], x: u32, y: u32| (2.0 * at(ow, px, x, y)[1] as f64 - at(ow, px, x - 1, y)[1] as f64 - at(ow, px, x + 1, y)[1] as f64).abs();
    let (mut det, mut moved, mut count) = ([[0f64; 2]; 2], [0f64; 2], [0u64; 2]);
    for y in 0..oh {
        for x in 1..ow - 1 {
            let i = (y * ow + x) as usize;
            let near = [alpha[i - 1], alpha[i], alpha[i + 1]];
            let part = if near.iter().all(|&a| a < 20) { 0 } else if near.iter().all(|&a| a > 235) { 1 } else { continue };
            det[part][0] += second(&orig, x, y);
            det[part][1] += second(&b, x, y);
            moved[part] += (0..3).map(|c| (b[i * 3 + c] as f64 - orig[i * 3 + c] as f64).abs()).sum::<f64>() / 3.0;
            count[part] += 1;
        }
    }
    let per = |v: f64, k: usize| v / count[k].max(1) as f64;
    let (bg_before, bg_after, fg_before, fg_after) = (per(det[0][0], 0), per(det[0][1], 0), per(det[1][0], 1), per(det[1][1], 1));
    let (bg_moved, fg_moved) = (per(moved[0], 0), per(moved[1], 1));
    eprintln!(
        "background ({} px): detail {bg_before:.2} -> {bg_after:.2}, pixels moved {bg_moved:.2}; subject ({} px): detail {fg_before:.2} -> {fg_after:.2}, moved {fg_moved:.2}",
        count[0], count[1]
    );
    assert!(count[0] > 1000 && count[1] > 1000, "the matte didn't split the photo");
    assert!(bg_after < bg_before * 0.5, "the background wasn't blurred: detail {bg_before:.2} -> {bg_after:.2}");
    assert!(fg_after > fg_before * 0.85, "the subject was blurred too: detail {fg_before:.2} -> {fg_after:.2}");
    assert!(bg_moved > 2.0 * fg_moved.max(0.5), "background moved {bg_moved:.2}, subject {fg_moved:.2}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_real_iphone_photo_comes_in_whole() {
    let (Some(kit), Some(ff)) = (kit(), ffmpeg()) else {
        eprintln!("skipped: ATLAS_PHOTO_KIT or ffmpeg isn't there");
        return;
    };
    let p = kit.join("iphone.heic");
    if !p.is_file() || !ffmpeg_reads_tiles(&ff) {
        eprintln!("skipped: no iphone.heic in the kit, or ffmpeg older than 8.1");
        return;
    }
    let seen = photo::look_at_photo(&ff, &p).unwrap();
    eprintln!("iphone.heic reads as {}x{}", seen.width, seen.height);
    assert!(seen.width > 512 * 4 && seen.height > 512 * 4, "only {}x{}: a tile, not the photo", seen.width, seen.height);
}

#[test]
fn real_photos_are_measured_for_tilt_and_say_how_sure() {
    let (Some(kit), Some(ff)) = (kit(), ffmpeg()) else {
        eprintln!("skipped: ATLAS_PHOTO_KIT or ffmpeg isn't there");
        return;
    };
    for name in ["portrait.jpg", "messi5.jpg", "iphone.heic"] {
        let p = kit.join(name);
        if !p.is_file() || (name.ends_with(".heic") && !ffmpeg_reads_tiles(&ff)) {
            continue;
        }
        let seen = photo::look_at_photo(&ff, &p).unwrap();
        let t = atlas::straighten::measure(&atlas::straighten::grey(&seen.small.rgb), seen.small.width as usize, seen.small.height as usize);
        eprintln!("{name}: {t:?}");
        if let Some(t) = t {
            assert!(t.fix.abs() <= atlas::straighten::MAX_TILT, "{name}: a tilt past the search range: {t:?}");
            assert!(t.confidence >= 0.0 && t.confidence <= 1.0, "{name}: confidence out of 0..1: {t:?}");
            // What it says follows what it measured (29 Sep 2026: this test
            // only printed the sentence, so a photo it was unsure of could
            // have been offered a fix and nothing would notice). A fix is
            // offered only when it is sure and the photo is off level; the
            // number in the offer is the one it measured.
            let said = atlas::straighten::offer(&t, name);
            eprintln!("  -> {said}");
            let offered = said.contains("Straighten it?");
            assert_eq!(offered, t.sure() && !t.level(), "{name}: {t:?} was answered with {said:?}");
            let unsure = said.contains("can't tell which way is level");
            assert_eq!(unsure, !t.sure(), "{name}: {t:?} was answered with {said:?}");
            if offered {
                let degrees = format!("{:.1}°", t.fix.abs());
                let names_it = said.contains(&degrees);
                assert!(names_it, "{name}: the offer doesn't give the {degrees} it measured: {said:?}");
            }
        }
    }
}
