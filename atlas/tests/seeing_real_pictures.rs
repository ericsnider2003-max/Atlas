//! Seeing, run on real pictures with the real models.
//!
//! Every other vision test feeds the decoders numbers shaped like a model's
//! output. That proves the decoding logic, not that the logic matches what
//! the models actually produce — `atlas-seeing.md` (11 Sep) said so plainly:
//! "No model has been run, so the box decoders and the per-model
//! preprocessing are written from published layouts and unverified."
//!
//! These run the OpenCV zoo files `atlas get` fetches on known pictures from
//! OpenCV's own sample set. They need the models and pictures on disk, so
//! they run when `ATLAS_SEEING_KIT` names a folder holding `models/` and the
//! pictures as raw RGB (`name.rgb`, with the size in the table below), and
//! say they were skipped otherwise.

use atlas::vision::{Album, Looking, Sight, VisionConfig};
use std::path::PathBuf;

fn kit() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var("ATLAS_SEEING_KIT").ok()?);
    p.join("models").join("objects.onnx").is_file().then_some(p)
}

fn picture(kit: &PathBuf, name: &str, w: usize, h: usize) -> Vec<u8> {
    let b = std::fs::read(kit.join(format!("{name}.rgb"))).expect("picture");
    assert_eq!(b.len(), w * h * 3, "{name} is not {w}x{h}");
    b
}

fn look(name: &str, w: usize, h: usize) -> Option<atlas::vision::Scene> {
    let Some(kit) = kit() else {
        eprintln!("skipped: ATLAS_SEEING_KIT isn't set");
        return None;
    };
    let mut eyes = Looking::open(&kit.join("models"));
    assert!(eyes.finding_faces.is_some() && eyes.naming_things.is_some() && eyes.describing.is_some(), "{eyes:?}");
    let rgb = picture(&kit, name, w, h);
    match eyes.look(&rgb, w, h, &VisionConfig { enabled: true, ..VisionConfig::default() }, &Album::default()) {
        Sight::Looked(scene) => {
            eprintln!("{name}: {}", scene.as_lines("me"));
            assert!(scene.could_not.is_empty(), "{name}: {:?}", scene.could_not);
            Some(scene)
        }
        Sight::Unread(why) => panic!("{name}: {why}"),
    }
}

#[test]
fn a_face_is_found_where_the_face_is() {
    // lena.jpg: one face, roughly in the upper middle of a 512x512 picture.
    let Some(scene) = look("lena", 512, 512) else { return };
    assert_eq!(scene.faces.len(), 1, "{:?}", scene.faces);
    let f = &scene.faces[0].area;
    let (cx, cy) = (f.x + f.width / 2.0, f.y + f.height / 2.0);
    assert!((0.4..0.75).contains(&cx) && (0.3..0.7).contains(&cy), "face centred at {cx},{cy}");
    assert!(f.width > 0.1 && f.width < 0.6, "face width {}", f.width);
}

#[test]
fn a_person_and_a_ball_are_named() {
    // messi5.jpg: a footballer and the ball.
    let Some(scene) = look("messi5", 548, 342) else { return };
    let names: Vec<&str> = scene.things.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"person"), "{names:?}");
    assert!(names.contains(&"sports ball"), "{names:?}");
    let person = scene.things.iter().find(|t| t.name == "person").unwrap();
    assert!(person.area.height > 0.4, "the person is most of the picture's height: {:?}", person.area);
}

#[test]
fn fruit_is_named_as_fruit() {
    let Some(scene) = look("fruits", 512, 480) else { return };
    let names: Vec<&str> = scene.things.iter().map(|t| t.name.as_str()).collect();
    assert!(names.iter().any(|n| ["apple", "orange", "banana"].contains(n)), "{names:?}");
}

#[test]
fn a_baboon_is_described_as_an_animal_not_a_face() {
    let Some(scene) = look("baboon", 512, 512) else { return };
    assert!(scene.faces.is_empty(), "a baboon is not a person's face: {:?}", scene.faces);
    let whole = scene.whole.as_ref().expect("a description");
    assert!(whole.name.to_lowercase().contains("baboon") || whole.name.to_lowercase().contains("mandrill"), "{}", whole.name);
}

#[test]
fn a_face_shown_once_is_known_again_and_a_stranger_is_not() {
    let Some(kit) = kit() else { return };
    let mut eyes = Looking::open(&kit.join("models"));
    let cfg = VisionConfig { enabled: true, ..VisionConfig::default() };
    let mut readings = |name: &str, w: usize, h: usize| -> Vec<Vec<f32>> {
        let rgb = picture(&kit, name, w, h);
        let Sight::Looked(scene) = eyes.look(&rgb, w, h, &cfg, &Album::default()) else { panic!("{name}") };
        scene.faces.iter().map(|f| eyes.face_reading(&rgb, w, h, &f.area, cfg.face_margin).unwrap()).collect()
    };
    let mut album = Album::default();
    album.remember_face("Lena", &readings("lena", 512, 512)[0], 0).unwrap();
    // The same person, mirrored and cropped differently: a different picture.
    let again = album.whose_face(&readings("lena2", 512, 512)[0], cfg.sure_enough_to_name_a_face, cfg.margin);
    assert_eq!(again, atlas::vision::Guess::Is("Lena".into(), match &again { atlas::vision::Guess::Is(_, s) => *s, _ => 0.0 }), "{again:?}");
    for stranger in readings("messi5", 548, 342) {
        let who = album.whose_face(&stranger, cfg.sure_enough_to_name_a_face, cfg.margin);
        assert_eq!(who, atlas::vision::Guess::NoIdea, "a stranger was named");
    }
}

#[test]
fn words_on_a_screen_are_read() {
    let Some(kit) = kit() else { return };
    let mut reader = atlas::words::Reader::open(&kit.join("models")).expect("both word models");
    let rgb = picture(&kit, "words", 800, 200);
    let read = reader.look(&rgb, 800, 200, &atlas::words::WordsConfig::default()).expect("read");
    let text = read.text().to_lowercase();
    eprintln!("read: {text:?}");
    for w in ["meeting", "moved", "three", "invoice", "4250"] {
        assert!(text.contains(w), "missed {w:?} in {text:?}");
    }
}

#[test]
fn a_line_is_split_into_its_words_at_the_gaps() {
    // No models needed: white line, two dark words with a wide gap, and a
    // narrow gap inside the first that is only between letters.
    let (w, h) = (200usize, 20usize);
    let mut rgb = vec![255u8; w * h * 3];
    for x in (10..30).chain(33..45).chain(80..120) {
        for y in 4..16 {
            let i = (y * w + x) * 3;
            rgb[i..i + 3].copy_from_slice(&[20, 20, 20]);
        }
    }
    let words = atlas::words::words_in(&rgb, w, h, (0, 0, w, h));
    assert_eq!(words.len(), 2, "{words:?}");
    assert!(words[0].0 <= 10 && words[0].0 + words[0].2 >= 45, "{words:?}");
    assert!(words[1].0 <= 80 && words[1].0 + words[1].2 >= 120, "{words:?}");
    // Light text on a dark line splits the same way.
    let dark: Vec<u8> = rgb.iter().map(|v| 255 - v).collect();
    assert_eq!(atlas::words::words_in(&dark, w, h, (0, 0, w, h)).len(), 2);
    // No gap: the line comes back whole.
    assert_eq!(atlas::words::words_in(&rgb, w, h, (5, 0, 45, h)), vec![(5, 0, 45, h)]);
}
