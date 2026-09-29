//! Reading words off a picture.
//!
//! Every number in this file came out of `docs/words_reference.py`, which was
//! actually run. None of it is a value I expected and wrote down.

use atlas::infer::{Kind, Layout};
use atlas::words::{
    blobs, crop_in, ctc, find, grow, looks_like_odds, reading_order, shares_a_line, strength,
    Lettering, Screenful, Strip, WordsConfig, ALPHABET, CLASSES,
};

fn cfg() -> WordsConfig {
    WordsConfig::default()
}

/// A twelve-by-nine map with two lumps on one line and one below, plus a
/// patch of barely-there noise.
///
/// Hand-built rather than random, so every expected box below can be read off
/// the picture by eye and a wrong answer looks wrong rather than plausible.
fn fixture() -> (Vec<f32>, usize, usize) {
    let (w, h) = (12usize, 9usize);
    let mut m = vec![0.02f32; w * h];
    let mut fill = |x0: usize, y0: usize, x1: usize, y1: usize, v: f32| {
        for y in y0..=y1 {
            for x in x0..=x1 {
                m[y * w + x] = v;
            }
        }
    };
    fill(1, 1, 3, 2, 0.90); // top line, left
    fill(6, 1, 8, 2, 0.80); // top line, right
    fill(2, 6, 5, 7, 0.70); // second line
    fill(10, 5, 11, 5, 0.31); // noise: over the ink threshold, under the score
    (m, w, h)
}

// ---------------------------------------------------------------------------
// Finding the lumps
// ---------------------------------------------------------------------------

#[test]
fn blobs_are_found_in_a_fixed_order_with_the_boxes_around_them() {
    let (m, w, h) = fixture();
    let found = blobs(&m, w, h, 0.3);
    let boxes: Vec<(usize, usize, usize, usize)> =
        found.iter().map(|s| (s.x, s.y, s.width, s.height)).collect();
    assert_eq!(boxes, vec![(1, 1, 3, 2), (6, 1, 3, 2), (10, 5, 2, 1), (2, 6, 4, 2)]);

    let scores: Vec<f32> = found.iter().map(|s| strength(&m, w, s)).collect();
    for (got, want) in scores.iter().zip([0.90f32, 0.80, 0.31, 0.70]) {
        assert!((got - want).abs() < 1e-6, "{got} vs {want}");
    }
}

#[test]
fn a_diagonal_stroke_is_one_lump_and_not_two() {
    // Four-connected would split this into three boxes and read one letter as
    // three. Eight-connected is not a detail — it is the difference between
    // reading 'W' and reading 'v v'.
    let (w, h) = (5usize, 5usize);
    let mut m = vec![0.0f32; w * h];
    for i in 0..5 {
        m[i * w + i] = 1.0;
    }
    let found = blobs(&m, w, h, 0.3);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!((found[0].width, found[0].height), (5, 5));
}

#[test]
fn a_box_is_grown_back_out_by_the_unclip_formula() {
    // The finder was trained on text regions that had been shrunk, so what it
    // reports is smaller than the letters. Skipping this does not fail — it
    // crops the tails off descenders and returns words that are nearly right.
    let (m, w, h) = fixture();
    let grown: Vec<(usize, usize, usize, usize)> = blobs(&m, w, h, 0.3)
        .iter()
        .map(|s| grow(s, 2.0, w, h))
        .map(|s| (s.x, s.y, s.width, s.height))
        .collect();
    assert_eq!(grown, vec![(0, 0, 5, 4), (5, 0, 5, 4), (9, 4, 3, 3), (1, 5, 6, 4)]);
}

#[test]
fn growing_a_box_at_the_edge_stops_at_the_edge() {
    let s = Strip { x: 0, y: 0, width: 4, height: 4, strength: 1.0 };
    let g = grow(&s, 2.0, 10, 10);
    assert_eq!((g.x, g.y), (0, 0), "can't grow past the picture");
    // area 16, perimeter 16, d = round(16*2/16) = 2
    assert_eq!((g.width, g.height), (6, 6));
}

#[test]
fn a_lump_that_passes_the_ink_threshold_can_still_fail_on_its_own_average() {
    // The two thresholds do different jobs and the second is the one that
    // matters. `ink` decides whether a single pixel counts; `keep_above` is
    // the box's own case for existing. The 0.31 patch clears the first and
    // fails the second, which is the whole reason there are two.
    let (m, w, h) = fixture();
    assert_eq!(blobs(&m, w, h, 0.3).len(), 4);
    let found = find(&m, w, h, &cfg());
    assert_eq!(found.strips.len(), 3, "{:?}", found.strips);
    assert!(
        !found.strips.iter().any(|s| s.x >= 9 && s.y >= 4),
        "the noise survived: {:?}",
        found.strips
    );
}

#[test]
fn a_box_is_scored_before_it_is_grown_and_not_after() {
    // Order of operations, and it is not the obvious one. Scoring the grown
    // box averages in the background it just swallowed, which drags every
    // score down — furthest for the smallest text, so the first thing it
    // would silently throw away is fine print.
    let (m, w, h) = fixture();
    let found = find(&m, w, h, &cfg());
    let strongest = found.strips.iter().map(|s| s.strength).fold(0.0f32, f32::max);
    assert!((strongest - 0.90).abs() < 1e-6, "{strongest}");

    // What the wrong order would have given for that same lump: the 3x2 lump
    // of 0.90 grown to 5x4, so six pixels of 0.90 and fourteen of 0.02.
    let wrong = (6.0 * 0.90 + 14.0 * 0.02) / 20.0;
    assert!(wrong < 0.5, "{wrong}");
    assert!(wrong < strongest, "scoring after growing is strictly worse: {wrong}");
}

#[test]
fn everything_comes_back_in_reading_order() {
    let (m, w, h) = fixture();
    let found = find(&m, w, h, &cfg());
    let boxes: Vec<(usize, usize, usize, usize)> =
        found.strips.iter().map(|s| (s.x, s.y, s.width, s.height)).collect();
    assert_eq!(boxes, vec![(0, 0, 5, 4), (5, 0, 5, 4), (1, 5, 6, 4)]);
    assert_eq!(found.over_the_limit, 0);
}

#[test]
fn the_left_word_wins_its_row_even_when_it_sits_lower() {
    // Sorting by y alone puts the second word of a line first whenever it
    // happens to sit a pixel higher, which is most of the time — and the
    // result reads as a shuffled word list while looking like it worked.
    let s = |x: usize, y: usize| Strip { x, y, width: 20, height: 8, strength: 0.9 };
    let order = reading_order(vec![s(50, 10), s(5, 11), s(5, 40)]);
    let xy: Vec<(usize, usize)> = order.iter().map(|r| (r.x, r.y)).collect();
    assert_eq!(xy, vec![(5, 11), (50, 10), (5, 40)]);
}

#[test]
fn a_full_stop_joins_the_line_it_belongs_to() {
    // Matched by overlap, not by the distance between centres: a full stop and
    // the word before it have very different heights and the same line.
    let word = Strip { x: 0, y: 10, width: 40, height: 20, strength: 0.9 };
    let stop = Strip { x: 42, y: 26, width: 3, height: 3, strength: 0.9 };
    assert!(shares_a_line(word.y, word.y + word.height, stop.y, stop.y + stop.height));
    let order = reading_order(vec![stop, word]);
    assert_eq!(order[0].x, 0, "the word comes first");
    assert_eq!(order.len(), 2);
}

#[test]
fn two_lines_are_two_lines() {
    assert!(shares_a_line(0, 10, 2, 12));
    assert!(!shares_a_line(0, 10, 9, 20), "one pixel of overlap is not a line");
    assert!(!shares_a_line(0, 10, 10, 20), "touching is not overlapping");
}

#[test]
fn the_limit_cuts_the_weakest_and_says_how_many() {
    // The count is separate from the boxes on purpose. Counting every blob the
    // finder produced would fold noise the threshold correctly rejected in
    // with real text the limit cut off, and report a healthy screen as one
    // Atlas couldn't keep up with.
    let (m, w, h) = fixture();
    let tight = WordsConfig { most: 2, ..cfg() };
    let found = find(&m, w, h, &tight);
    assert_eq!(found.strips.len(), 2);
    assert_eq!(found.over_the_limit, 1);
    // The two strongest survived, and they are the top line.
    let boxes: Vec<(usize, usize)> = found.strips.iter().map(|s| (s.x, s.y)).collect();
    assert_eq!(boxes, vec![(0, 0), (5, 0)]);
}

#[test]
fn an_empty_map_finds_nothing_rather_than_panicking() {
    assert!(blobs(&[], 0, 0, 0.3).is_empty());
    assert!(blobs(&[0.9, 0.9], 8, 8, 0.3).is_empty(), "too few values for the size");
    assert!(find(&[], 0, 0, &cfg()).strips.is_empty());
}

// ---------------------------------------------------------------------------
// Turning a strip into letters
// ---------------------------------------------------------------------------

/// One timestep that picks `class` with a raw score of 4.
fn step(class: usize) -> Vec<f32> {
    let mut row = vec![0.0f32; CLASSES];
    row[class] = 4.0;
    row
}

fn steps(classes: &[usize]) -> Vec<f32> {
    classes.iter().flat_map(|c| step(*c)).collect()
}

fn ix(c: char) -> usize {
    ALPHABET.chars().position(|a| a == c).unwrap() + 1
}

/// softmax(4 against thirty-six zeros) = e^4 / (e^4 + 36).
const ONE_STRONG_STEP: f32 = 0.602_641;

#[test]
fn a_plain_word_decodes() {
    let got = ctc(&steps(&[ix('a'), ix('t')]), 2).unwrap();
    assert_eq!(got.text, "at");
    assert!((got.sure - ONE_STRONG_STEP).abs() < 1e-5, "{}", got.sure);
}

#[test]
fn a_repeat_with_nothing_between_it_collapses() {
    let got = ctc(&steps(&[ix('a'), ix('a'), ix('t')]), 3).unwrap();
    assert_eq!(got.text, "at");
}

#[test]
fn a_repeat_split_by_a_blank_survives() {
    // This is the entire reason the blank class exists. Without it there is no
    // way to write 'oo', and 'cool' comes back as 'col'.
    let got = ctc(&steps(&[ix('o'), 0, ix('o')]), 3).unwrap();
    assert_eq!(got.text, "oo");

    let cool = ctc(&steps(&[ix('c'), ix('o'), 0, ix('o'), ix('l'), ix('l')]), 6).unwrap();
    assert_eq!(cool.text, "cool");
}

#[test]
fn blanks_at_either_end_are_dropped() {
    let got = ctc(&steps(&[0, 0, ix('h'), ix('i'), 0]), 5).unwrap();
    assert_eq!(got.text, "hi");
    assert!((got.sure - ONE_STRONG_STEP).abs() < 1e-5, "{}", got.sure);
}

#[test]
fn confidence_counts_only_the_steps_that_produced_a_letter() {
    // A short word in a long window is mostly blank, and blanks are the easy
    // part. Averaging over every step reports ninety per cent for a word that
    // was a guess.
    let mut vals = steps(&[ix('h'), ix('i')]);
    for _ in 0..20 {
        // A blank step the model is completely certain about.
        let mut row = vec![0.0f32; CLASSES];
        row[0] = 20.0;
        vals.extend(row);
    }
    let got = ctc(&vals, 22).unwrap();
    assert_eq!(got.text, "hi");
    assert!(
        (got.sure - ONE_STRONG_STEP).abs() < 1e-5,
        "the twenty certain blanks leaked in: {}",
        got.sure
    );
}

#[test]
fn nothing_read_is_no_confidence_rather_than_full_confidence() {
    let got = ctc(&steps(&[0, 0, 0]), 3).unwrap();
    assert_eq!(got.text, "");
    assert_eq!(got.sure, 0.0);
}

#[test]
fn values_that_are_already_odds_are_not_softmaxed_a_second_time() {
    // The failure worth having a test for. Softmaxing something already
    // softmaxed does not fail: it flattens everything toward one
    // thirty-seventh and reports seven per cent for a word read perfectly. The
    // text stays right and the number beside it goes wrong — and because the
    // number is what `sure_enough` filters on, the symptom is "Atlas can't
    // read my screen", which nobody traces back to here.
    let mut vals: Vec<f32> = Vec::new();
    for class in [ix('a'), 0, ix('t')] {
        let mut row = vec![0.01f32 / (CLASSES as f32 - 1.0); CLASSES];
        row[class] = 0.99;
        vals.extend(row);
    }
    assert!(looks_like_odds(&vals, 3, CLASSES));
    let got = ctc(&vals, 3).unwrap();
    assert_eq!(got.text, "at");
    assert!((got.sure - 0.99).abs() < 1e-5, "{}", got.sure);
    // What a second softmax would have said instead, from the reference run.
    assert!((got.sure - 0.069_539).abs() > 0.5, "that is the softmaxed-twice number");
}

#[test]
fn raw_scores_are_recognised_as_raw_scores() {
    let vals = steps(&[ix('a')]);
    assert!(!looks_like_odds(&vals, 1, CLASSES), "a row of 4.0 is not a distribution");
    let negative: Vec<f32> = vec![-1.0; CLASSES];
    assert!(!looks_like_odds(&negative, 1, CLASSES));
}

#[test]
fn a_reader_with_the_wrong_number_of_classes_is_an_error_and_not_a_guess() {
    // Reading garbage out of a model that isn't the one this was written for
    // produces real-looking words. An error names the problem.
    let short = vec![0.0f32; CLASSES - 1];
    assert!(ctc(&short, 1).is_err());
    assert!(ctc(&[], 0).is_err());
}

#[test]
fn the_alphabet_and_the_class_count_agree() {
    // Off by one here does not fail. It returns real words with every letter
    // shifted by one, which reads as a broken model rather than as an index.
    assert_eq!(ALPHABET.chars().count() + 1, CLASSES);
    assert_eq!(ALPHABET.chars().count(), 36);
    assert!(ALPHABET.starts_with('0'));
    assert!(ALPHABET.ends_with('z'));
    assert!(!ALPHABET.chars().any(|c| c.is_uppercase()), "the model has no capitals");
}

// ---------------------------------------------------------------------------
// What a whole screenful adds up to
// ---------------------------------------------------------------------------

fn said(text: &str, sure: f32) -> Lettering {
    Lettering { at: atlas::vision::Patch::new(0.0, 0.0, 0.1, 0.05), text: text.into(), sure }
}

#[test]
fn a_screenful_weights_its_confidence_by_how_long_each_word_is() {
    // One confident 'a' should not outvote a doubtful eleven-character word.
    let s = Screenful {
        lettering: vec![said("a", 1.0), said("transaction", 0.5)],
        unsure: 0,
        unread: 0,
    };
    let want = (1.0 * 1.0 + 0.5 * 11.0) / 12.0;
    assert!((s.sure() - want).abs() < 1e-5, "{} vs {want}", s.sure());
    assert!(s.sure() < 0.6, "the long doubtful word dominates, as it should");
}

#[test]
fn a_half_read_screen_is_refused_rather_than_handed_on() {
    // OCR does not fail by returning an error. It fails by returning
    // confident, plausible, wrong text, and an assistant that acts on that is
    // worse than one that says it couldn't read the screen.
    let poor = Screenful { lettering: vec![said("acount", 0.55)], unsure: 12, unread: 0 };
    assert!(!poor.worth_acting_on());
    assert!(poor.spoken().contains("rather not act on it"), "{}", poor.spoken());
    assert!(poor.spoken().contains("12"), "the dropped ones are said: {}", poor.spoken());

    let good = Screenful {
        lettering: vec![said("balance", 0.95), said("available", 0.92)],
        unsure: 0,
        unread: 0,
    };
    assert!(good.worth_acting_on());
    assert_eq!(good.text(), "balance available");
}

#[test]
fn one_word_is_never_enough_however_sure_it_is() {
    // A single word at ninety-nine per cent is the shape a misread takes: the
    // model is certain about the one thing it found and found nothing else.
    let one = Screenful { lettering: vec![said("send", 0.99)], unsure: 0, unread: 0 };
    assert!(!one.worth_acting_on());
}

#[test]
fn a_screen_with_more_on_it_than_was_read_says_so() {
    let s = Screenful {
        lettering: vec![said("balance", 0.95), said("available", 0.92)],
        unsure: 0,
        unread: 40,
    };
    assert!(s.worth_acting_on());
    assert!(s.spoken().contains("40"), "{}", s.spoken());
    assert!(s.spoken().contains("more on screen"), "{}", s.spoken());
}

#[test]
fn an_empty_screenful_says_nothing_was_there() {
    let s = Screenful::default();
    assert_eq!(s.sure(), 0.0);
    assert_eq!(s.words(), 0);
    assert!(!s.worth_acting_on());
    assert!(s.spoken().contains("couldn't find any words"), "{}", s.spoken());
}

// ---------------------------------------------------------------------------
// The two models, and what they want fed to them
// ---------------------------------------------------------------------------

#[test]
fn reading_needs_both_models_and_says_so() {
    let nowhere = std::path::Path::new("/definitely/not/here");
    let missing = atlas::infer::whats_missing(nowhere, &Kind::for_reading());
    assert_eq!(missing.len(), 2, "a finder with no reader is not half the feature");
    assert!(!atlas::words::Reader::installed(nowhere));
    let spoken = atlas::infer::spoken(&missing);
    assert!(spoken.contains("finding words"), "{spoken}");
    assert!(spoken.contains("reading those words"), "{spoken}");
}

#[test]
fn the_reader_is_fed_one_grey_channel_and_not_three() {
    // Handing it three is not a shape error it would report. It is a tensor
    // three times the size it expects, and the engine's complaint is about
    // dimensions and says nothing about colour.
    let r = Kind::TextRead.recipe();
    assert_eq!(r.layout, Layout::Grey);
    assert_eq!(r.layout.channels(), 1);
    assert_eq!(r.values(), 100 * 32);
    assert_eq!(r.shape(), [1, 1, 32, 100]);
    assert!(r.width > r.height, "a strip of a line, not a square of picture");
}

#[test]
fn the_finder_is_fed_a_seven_thirty_six_square_blue_first() {
    let r = Kind::TextFind.recipe();
    assert_eq!(r.shape(), [1, 3, 736, 736]);
    assert!(r.blue_first, "PaddleOCR trained it on blue-first pictures");
    // 255 x the ImageNet standard deviations, because OpenCV multiplies by
    // 1/255/std where Atlas divides.
    for (got, want) in r.deviation.iter().zip([58.395f32, 57.12, 57.375]) {
        assert!((got - want).abs() < 1e-3, "{got} vs {want}");
    }
    assert_eq!(r.mean, [123.675, 116.28, 103.53]);
}

#[test]
fn one_pixel_through_each_recipe_lands_where_the_reference_said() {
    // rgb(200, 100, 50), from docs/words_reference.py.
    let pixel: Vec<u8> = vec![200, 100, 50];
    let finder = Kind::TextFind.recipe();
    let fitted = atlas::infer::fit(&pixel, 1, 1, 1, 1);
    let one = atlas::infer::Recipe { width: 1, height: 1, ..finder };
    let out = atlas::infer::arrange(&fitted, &one);
    assert_eq!(out.len(), 3);
    for (got, want) in out.iter().zip([-1.261_666f32, -0.285_014, 1.681_394]) {
        assert!((got - want).abs() < 1e-4, "{got} vs {want}");
    }

    let reader = Kind::TextRead.recipe();
    let grey = atlas::infer::Recipe { width: 1, height: 1, ..reader };
    let out = atlas::infer::arrange(&fitted, &grey);
    assert_eq!(out.len(), 1, "one channel out, whatever went in");
    // 200*0.299 + 100*0.587 + 50*0.114 = 124.2; (124.2 - 127.5) / 127.5
    assert!((out[0] - -0.025_882).abs() < 1e-4, "{}", out[0]);
}

#[test]
fn a_fractional_box_becomes_whole_pixels_or_nothing() {
    let p = atlas::vision::Patch::new(0.25, 0.5, 0.5, 0.25);
    assert_eq!(crop_in(&p, 100, 80), Some((25, 40, 50, 20)));

    // A box that landed entirely in the letterbox padding comes back negative,
    // and there is nothing there to read.
    let outside = atlas::vision::Patch::new(-0.4, -0.4, 0.1, 0.1);
    assert_eq!(crop_in(&outside, 100, 80), None);
    assert_eq!(crop_in(&p, 0, 0), None);
}

#[test]
fn the_two_reading_models_do_not_collide_with_any_other() {
    let all = Kind::all();
    assert_eq!(all.len(), 8);
    let files: Vec<&str> = all.iter().map(|k| k.file()).collect();
    for (i, a) in files.iter().enumerate() {
        for b in files.iter().skip(i + 1) {
            assert_ne!(a, b);
        }
    }
    assert!(files.contains(&"text_find.onnx"));
    assert!(files.contains(&"text_read.onnx"));
    for k in Kind::for_reading() {
        assert!(!Kind::for_seeing().contains(&k), "{k:?} is in two install lists");
        assert!(!Kind::for_hands().contains(&k), "{k:?} is in two install lists");
    }
}

// ---------------------------------------------------------------------------
// Getting the pixels in the first place
// ---------------------------------------------------------------------------

#[test]
fn the_screen_is_grabbed_as_raw_bytes_and_never_as_a_file() {
    // The old route wrote a PNG to disk and handed the filename to another
    // program. Raw bytes mean nothing touches the disk — and nothing is left
    // behind afterwards for somebody to find.
    let a = atlas::words::capture_args(100, 200, 800, 400);
    let joined = a.join(" ");
    assert!(joined.contains("-f rawvideo"), "{joined}");
    assert!(joined.contains("-pix_fmt rgb24"), "{joined}");
    assert!(joined.contains("800x400"), "{joined}");
    assert!(joined.contains("-offset_x 100"), "{joined}");
    assert_eq!(a.last().map(|s| s.as_str()), Some("-"), "to stdout, not to a file");
    assert!(!joined.contains(".png"), "no file anywhere: {joined}");
    // One frame. Without this it grabs until it is stopped.
    assert!(joined.contains("-frames:v 1"), "{joined}");
}

#[test]
fn a_short_read_is_refused_rather_than_read_anyway() {
    // The failure this exists for: ffmpeg hands back a stream, and with no
    // expected count a truncated one looks exactly like a whole one. Atlas
    // would read the top two-thirds of the screen, find words, and never
    // mention the third it did not get.
    assert!(atlas::words::whole_picture(800 * 400 * 3, 800, 400).is_ok());
    let short = atlas::words::whole_picture(800 * 400 * 3 - 9000, 800, 400);
    assert!(short.is_err());
    let said = format!("{}", short.unwrap_err());
    assert!(said.contains("half a sentence"), "{said}");
    assert!(atlas::words::whole_picture(1, 800, 400).is_err());
    assert!(atlas::words::whole_picture(9_999_999, 8, 8).is_err(), "too many is wrong too");
}

/// A PNG header: the signature, a chunk length, IHDR, then width and height.
fn png_header(w: u32, h: u32) -> Vec<u8> {
    let mut v = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    v.extend_from_slice(&13u32.to_be_bytes());
    v.extend_from_slice(b"IHDR");
    v.extend_from_slice(&w.to_be_bytes());
    v.extend_from_slice(&h.to_be_bytes());
    v.extend_from_slice(&[8, 2, 0, 0, 0]);
    v
}

/// A JPEG header with two skippable markers before the start-of-frame, so the
/// walk has something to walk. `sof` picks baseline (0xC0) or progressive
/// (0xC2) — a phone makes the second one.
fn jpeg_header(w: u16, h: u16, sof: u8) -> Vec<u8> {
    let mut v = vec![0xFF, 0xD8];
    v.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x10]); // APP0, 14 bytes of payload
    v.extend_from_slice(&[0u8; 14]);
    v.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x05]); // a quantisation table
    v.extend_from_slice(&[0u8; 3]);
    v.extend_from_slice(&[0xFF, sof, 0x00, 0x11, 0x08]); // start of frame
    v.extend_from_slice(&h.to_be_bytes());
    v.extend_from_slice(&w.to_be_bytes());
    v.extend_from_slice(&[3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
    v
}

#[test]
fn a_picture_states_its_own_size_in_its_first_few_bytes() {
    // Checked against nineteen real files written by Pillow — PNG, baseline
    // JPEG, progressive JPEG and grayscale JPEG at six sizes — with no
    // mismatches. These fixtures are the same shapes, hand-built so the
    // offsets are visible.
    use atlas::words::picture_size;
    assert_eq!(picture_size(&png_header(2560, 1392)), Some((2560, 1392)));
    assert_eq!(picture_size(&png_header(1, 1)), Some((1, 1)));
    assert_eq!(picture_size(&jpeg_header(640, 480, 0xC0)), Some((640, 480)));
    // Progressive. Checking only 0xC0 fails on exactly the pictures a phone
    // makes, and fails by returning nothing rather than by being wrong — so
    // it would look like "Atlas can't read photos from my phone".
    assert_eq!(picture_size(&jpeg_header(2560, 1392, 0xC2)), Some((2560, 1392)));
    assert_eq!(picture_size(&jpeg_header(300, 7, 0xC1)), Some((300, 7)));
}

#[test]
fn something_that_is_not_a_picture_is_not_given_a_size() {
    use atlas::words::picture_size;
    assert_eq!(picture_size(&[]), None);
    assert_eq!(picture_size(b"hello world, definitely not a picture"), None);
    assert_eq!(picture_size(&[0xFF, 0xD8, 0x00, 0x00]), None, "jpeg start, then rubbish");
    let mut sig_only = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    sig_only.extend_from_slice(&[0u8; 16]);
    assert_eq!(picture_size(&sig_only), None, "png signature with no IHDR");
    // Zero-sized is refused: it would divide by nothing downstream.
    assert_eq!(picture_size(&png_header(0, 100)), None);
    assert_eq!(picture_size(&jpeg_header(100, 0, 0xC0)), None);
}

#[test]
fn a_picture_file_is_decoded_by_the_same_tool_and_to_the_same_format() {
    // One format on the way in means one path afterwards. A second decoder
    // for files would be a second place for the channel order to be wrong.
    let a = atlas::words::picture_args("C:/Users/erics/shot.png");
    let joined = a.join(" ");
    assert!(joined.contains("-pix_fmt rgb24"), "{joined}");
    assert!(joined.contains("C:/Users/erics/shot.png"), "{joined}");
    assert_eq!(a.last().map(|s| s.as_str()), Some("-"));
}
