//! The video studio (why-stale idea 6, 1 Oct 2026): dead air cut,
//! captions, thumbnails, a title -- on a copy, with real ffmpeg where this
//! machine has it.

use atlas::studio::{cut_args_with_audio, keep_spans, silence_args, silences, title_and_description, KEEP_PAD};

#[cfg(windows)]
fn installed_video_tool(name: &str) -> atlas::tools::ExternalTool {
    let command = format!("{}/Atlas/tools/ffmpeg/{name}.exe", std::env::var("LOCALAPPDATA").unwrap());
    assert!(std::path::Path::new(&command).is_file(), "installed {name} is required for this proof");
    atlas::tools::ExternalTool { command, timeout_secs: 15, ..Default::default() }
}

#[cfg(windows)]
#[test]
fn installed_tools_prepare_silent_footage_and_thumbnails_without_overwriting_previous_reviews() {
    use atlas::studio::{probe, reserve_folder, run_tool, cut_args_with_audio, thumbnail_fallback_args};
    let folder = std::env::temp_dir().join(format!("atlas-studio-proof-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir(&folder).unwrap();
    let input = folder.join("camera.mp4");
    let ffmpeg = installed_video_tool("ffmpeg");
    let ffprobe = installed_video_tool("ffprobe");
    let args = ["-hide_banner", "-n", "-f", "lavfi", "-i", "color=c=blue:s=320x240:d=2:r=25", "-c:v", "libx264", "-pix_fmt", "yuv420p"].iter().map(|s| s.to_string()).chain([input.display().to_string()]).collect();
    run_tool(&ffmpeg, args, &|| false).unwrap();
    let original = std::fs::read(&input).unwrap();
    let first = reserve_folder(&input).unwrap();
    std::fs::write(first.join("review.txt"), "previous review").unwrap();
    let second = reserve_folder(&input).unwrap();
    assert_ne!(first, second);
    let output = second.join("cut.mp4");
    let args = atlas::edit::probe_args(&input.display().to_string());
    let read = run_tool(&ffprobe, args, &|| false).unwrap();
    let (before, audio) = probe(&String::from_utf8_lossy(&read.stdout)).unwrap();
    assert!(!audio);
    run_tool(&ffmpeg, cut_args_with_audio(&input.display().to_string(), &[(0.0, before)], &output.display().to_string(), audio), &|| false).unwrap();
    let read = run_tool(&ffprobe, atlas::edit::probe_args(&output.display().to_string()), &|| false).unwrap();
    let (after, audio) = probe(&String::from_utf8_lossy(&read.stdout)).unwrap();
    assert!(!audio);
    assert!((after - before).abs() < 0.2);
    let pattern = second.join("thumbnail-%02d.jpg").display().to_string();
    // The installed encoder may reject a zero-frame scene selection. The
    // production journey treats that as unavailable and samples the clip.
    let _ = run_tool(&ffmpeg, atlas::studio::thumb_args(&output.display().to_string(), &pattern), &|| false);
    assert!(!second.join("thumbnail-01.jpg").exists(), "still shot has no scene changes");
    run_tool(&ffmpeg, thumbnail_fallback_args(&output.display().to_string(), &pattern, after), &|| false).unwrap();
    assert!(second.join("thumbnail-01.jpg").metadata().unwrap().len() > 0);
    assert_eq!(std::fs::read(&input).unwrap(), original);
    assert_eq!(std::fs::read_to_string(first.join("review.txt")).unwrap(), "previous review");
    assert_eq!(atlas::studio::requested_aspect("for YouTube"), None, "provider name does not silently reframe");
    assert_eq!(atlas::studio::requested_aspect("make it vertical"), Some((1080, 1920)));
    let aspect = second.join("vertical.mp4");
    run_tool(&ffmpeg, atlas::studio::aspect_args(&output.display().to_string(), &aspect.display().to_string(), 180, 320), &|| false).unwrap();
    let check = run_tool(&ffprobe, atlas::edit::probe_args(&aspect.display().to_string()), &|| false).unwrap();
    assert_eq!(atlas::studio::dimensions(&String::from_utf8_lossy(&check.stdout)), Some((180, 320)));
    let previous_cut = std::fs::read(&output).unwrap();
    assert!(run_tool(&ffmpeg, cut_args_with_audio(&input.display().to_string(), &[(0.0, before)], &output.display().to_string(), false), &|| false).is_err(), "existing result is never overwritten");
    assert_eq!(std::fs::read(&output).unwrap(), previous_cut);
    eprintln!("Installed-tool review artifact: {}", second.display());
}

#[cfg(windows)]
#[test]
fn installed_ffmpeg_stops_during_work_and_a_deadline_ends_a_stalled_job() {
    use atlas::studio::run_tool;
    let args = || ["-hide_banner", "-re", "-f", "lavfi", "-i", "color=c=blue:s=64x64:r=10", "-f", "null", "-"].iter().map(|s| s.to_string()).collect();
    let mut tool = installed_video_tool("ffmpeg");
    let start = std::time::Instant::now();
    assert_eq!(run_tool(&tool, args(), &|| start.elapsed().as_millis() > 150).unwrap_err(), "stopped");
    assert!(start.elapsed().as_secs_f64() < 2.0);
    tool.timeout_secs = 1;
    let start = std::time::Instant::now();
    let error = run_tool(&tool, args(), &|| false).unwrap_err();
    assert!(error.contains("still running after 1s"), "{error}");
    assert!(start.elapsed().as_secs_f64() < 3.0);
}

#[test]
fn silences_are_read_off_ffmpegs_output_and_shrunk_to_a_breath() {
    let err = "[silencedetect @ 0x1] silence_start: 2.5\n[silencedetect @ 0x1] silence_end: 6.5 | silence_duration: 4\n\
               [silencedetect @ 0x1] silence_start: 9";
    let s = silences(err, 12.0);
    assert_eq!(s, vec![(2.5, 6.5), (9.0, 12.0)]);
    let keep = keep_spans(&s, 12.0);
    assert_eq!(keep, vec![(0.0, 2.5 + KEEP_PAD), (6.5 - KEEP_PAD, 9.0 + KEEP_PAD)]);
    let args = cut_args_with_audio("in.mp4", &keep, "out.mp4", true);
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
    let cut = std::process::Command::new("ffmpeg").args(cut_args_with_audio(&src.display().to_string(), &keep, &out.display().to_string(), true)).output().unwrap();
    assert!(cut.status.success(), "{}", String::from_utf8_lossy(&cut.stderr));
    let probe = std::process::Command::new("ffprobe").args(atlas::edit::probe_args(&out.display().to_string())).output().unwrap();
    let len = atlas::edit::duration_from_probe(&String::from_utf8_lossy(&probe.stdout)).unwrap();
    assert!((5.0..7.0).contains(&len), "about 6.5 s left of 10: {len}");
    let _ = std::fs::remove_dir_all(&dir);
}
