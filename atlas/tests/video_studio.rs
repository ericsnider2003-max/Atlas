//! The video studio (why-stale idea 6, 1 Oct 2026): dead air cut,
//! captions, thumbnails, a title -- on a copy, with real ffmpeg where this
//! machine has it.

use atlas::studio::{cut_args, keep_spans, ready_said, silence_args, silences, title_and_description, KEEP_PAD};

#[test]
fn silences_are_read_off_ffmpegs_output_and_shrunk_to_a_breath() {
    let err = "[silencedetect @ 0x1] silence_start: 2.5\n[silencedetect @ 0x1] silence_end: 6.5 | silence_duration: 4\n\
               [silencedetect @ 0x1] silence_start: 9";
    let s = silences(err, 12.0);
    assert_eq!(s, vec![(2.5, 6.5), (9.0, 12.0)]);
    let keep = keep_spans(&s, 12.0);
    assert_eq!(keep, vec![(0.0, 2.5 + KEEP_PAD), (6.5 - KEEP_PAD, 9.0 + KEEP_PAD)]);
    let args = cut_args("in.mp4", &keep, "out.mp4");
    assert!(args.iter().any(|a| a.contains("between(t,0.000,2.750)+between(t,6.250,9.250)")), "{args:?}");
}

#[test]
fn a_title_the_video_never_said_is_not_kept() {
    let said = "today we bake sourdough bread from scratch in the new oven";
    let ok = title_and_description("TITLE: Baking sourdough bread from scratch\nDESCRIPTION: A loaf in the new oven.", said);
    assert_eq!(ok, Some(("Baking sourdough bread from scratch".into(), "A loaf in the new oven.".into())));
    assert_eq!(title_and_description("TITLE: You won't believe this secret trick\nDESCRIPTION: Wow.", said), None);
    assert_eq!(title_and_description("no format at all", said), None);
}

#[test]
fn the_readout_says_what_was_made_and_that_nothing_was_posted() {
    let s = ready_said("C:\\clips\\bakery - ready", 600.0, 480.0, true, 3, Some("Baking sourdough"));
    assert_eq!(
        s,
        "Your video's ready in C:\\clips\\bakery - ready: 10 min down to 8 min with the dead air cut. Captions are beside it as an .srt. \
         3 thumbnail frames to choose from. Suggested title: \"Baking sourdough\" -- the description's in the folder. \
         Nothing's been posted; the original is untouched."
    );
}

/// With real ffmpeg: a clip with a tone, four seconds of silence and a tone
/// again comes out about four seconds shorter, picture and sound together.
#[test]
fn real_ffmpeg_cuts_the_dead_air_out_of_a_real_clip() {
    if std::process::Command::new("ffmpeg").arg("-version").output().is_err() {
        eprintln!("no ffmpeg here -- skipped");
        return;
    }
    let dir = std::env::temp_dir().join(format!("atlas-studio-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("clip.mp4");
    let made = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-y", "-f", "lavfi", "-i", "color=c=blue:s=320x240:d=10:r=25", "-f", "lavfi", "-i",
               "sine=f=440:d=10", "-filter_complex", "[1:a]volume=enable='between(t,3,7)':volume=0[a]", "-map", "0:v", "-map", "[a]",
               "-shortest", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac"])
        .arg(&src)
        .output()
        .unwrap();
    assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));
    let found = std::process::Command::new("ffmpeg").args(silence_args(&src.display().to_string())).output().unwrap();
    let s = silences(&String::from_utf8_lossy(&found.stderr), 10.0);
    assert_eq!(s.len(), 1, "{s:?}");
    let keep = keep_spans(&s, 10.0);
    let out = dir.join("cut.mp4");
    let cut = std::process::Command::new("ffmpeg").args(cut_args(&src.display().to_string(), &keep, &out.display().to_string())).output().unwrap();
    assert!(cut.status.success(), "{}", String::from_utf8_lossy(&cut.stderr));
    let probe = std::process::Command::new("ffprobe").args(atlas::edit::probe_args(&out.display().to_string())).output().unwrap();
    let len = atlas::edit::duration_from_probe(&String::from_utf8_lossy(&probe.stdout)).unwrap();
    assert!((5.0..7.0).contains(&len), "about 6.5 s left of 10: {len}");
    let _ = std::fs::remove_dir_all(&dir);
}
