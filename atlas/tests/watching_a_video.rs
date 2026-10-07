//! "Watch this video" on this computer (6 Oct 2026, taken from Muse Spark,
//! which reads video): frames to the local picture reader, words to the
//! local transcriber, and the model here answering from both.

use atlas::watchvideo;

#[test]
fn frames_are_spread_through_the_video_and_never_at_its_edges() {
    assert_eq!(watchvideo::frame_times(60.0, 6), vec![5.0, 15.0, 25.0, 35.0, 45.0, 55.0]);
    assert!(watchvideo::frame_times(0.0, 6).is_empty(), "no length, no frames");
    assert!(watchvideo::frame_times(f64::NAN, 6).is_empty());
    let a = watchvideo::frame_args("in.mp4", 5.0, "f.png");
    assert_eq!(&a[2..4], &["-ss".to_string(), "5.00".to_string()], "seek before opening: fast on long videos");
    assert!(a.windows(2).any(|w| w[0] == "-frames:v" && w[1] == "1"));
    assert_eq!(watchvideo::at_clock(65.4), "1:05");
}

#[test]
fn the_answer_is_built_from_the_frames_and_the_words_in_order() {
    let frames = vec![(5.0, "A dog runs on a beach.".to_string()), (65.0, "The dog fetches a stick.".to_string())];
    let p = watchvideo::answer_prompt("is the dog happy?", &frames, "good boy, fetch!");
    assert!(p.starts_with("Question: is the dog happy?"));
    assert!(p.find("[0:05] A dog").unwrap() < p.find("[1:05] The dog").unwrap());
    assert!(p.contains("good boy, fetch!"));
    let p = watchvideo::answer_prompt("", &frames, "");
    assert!(p.contains("What happens in this video?") && !p.contains("What is said"), "{p}");
    let long = "word ".repeat(3000);
    assert!(watchvideo::answer_prompt("", &frames, &long).contains("(the start)"), "a long transcript is cut, and says so");
}

#[test]
fn watching_is_asked_for_in_plain_words() {
    assert!(watchvideo::asks("watch this video C:\\clips\\trip.mp4"));
    assert!(watchvideo::asks("What happens in this video: \"D:\\a b\\c.mov\""));
    assert!(watchvideo::asks("summarize the video C:\\x.mp4"));
    assert!(!watchvideo::asks("edit this video"), "editing is the editor's");
    assert!(!watchvideo::asks("I watched a video yesterday"));
}

#[test]
fn the_daemon_asks_which_video_and_checks_it_exists() {
    let dir = std::env::temp_dir().join(format!("atlas-watch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let p = atlas::platform::mock::MockPlatform::new(vec![atlas::platform::Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let c = atlas::config::Config::load(std::path::Path::new("config")).unwrap();
    let mut d = atlas::daemon::Daemon::new(&c, &p, None, atlas::store::Store::new(dir.clone()), atlas::proactive::Proactive::new(atlas::proactive::ProactiveConfig::default()));
    let now = atlas::store::now();
    let r = d.turn("watch this video", now);
    assert!(r.contains("Which video?"), "{r}");
    let r = d.turn("watch this video C:\\nowhere\\gone.mp4", now);
    assert!(r.contains("can't find"), "{r}");
    // A real file, and no picture reader in the test install: said plainly.
    let clip = dir.join("clip.mp4");
    std::fs::write(&clip, b"not really a video").unwrap();
    let r = d.turn(&format!("watch this video \"{}\" what happens?", clip.display()), now);
    assert!(r.contains("watch") && (r.contains("can't") || r.contains("Watching")), "{r}");
    assert_eq!(std::fs::read(&clip).unwrap(), b"not really a video", "watching never touches the video itself");
    let _ = std::fs::remove_dir_all(dir);
}
