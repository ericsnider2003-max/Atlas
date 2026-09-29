//! Watching a video rather than only listening to it.
//!
//! The naive version is a screenshot every few seconds, and it fails in both
//! directions at once: hundreds of near-identical pictures of a static slide
//! filling the disk, and a missed frame at the one second something appeared.
//! Partial context, at the cost of space.

use atlas::viewing::{
    frame_times, one_per_moment, read_timed, retell, same_screen, scene_times, weave, weave_seen,
    where_to_look, Moment, Seen, Spoken, ViewConfig,
};

// ---------------------------------------------------------------------------
// Which frames are worth looking at
// ---------------------------------------------------------------------------

const FFMPEG_SAID: &str = "\
[Parsed_showinfo_1 @ 0x55] n:0 pts:0 pts_time:0 duration:1 fmt:rgb24
[Parsed_showinfo_1 @ 0x55] n:1 pts:301 pts_time:12.54 duration:1 fmt:rgb24
frame=  2 fps=0.0 q=-0.0 size=N/A time=00:00:12.54 bitrate=N/A
[Parsed_showinfo_1 @ 0x55] n:2 pts:900 pts_time:37.5 duration:1 fmt:rgb24
";

#[test]
fn the_times_the_picture_changed_are_read_out_of_what_ffmpeg_said() {
    let times = scene_times(FFMPEG_SAID, 40);
    assert_eq!(times, vec![0.0, 12.54, 37.5]);
}

#[test]
fn ffmpeg_talking_about_something_else_is_ignored() {
    let times = scene_times("frame= 12 fps=0.0 time=00:00:03.00 bitrate=N/A\n", 40);
    assert!(
        times.is_empty(),
        "ffmpeg says a great deal that is not a scene change"
    );
}

#[test]
fn two_scene_changes_in_the_same_half_second_are_one_moment() {
    let told = "pts_time:10.0\npts_time:10.2\npts_time:22.0\n";
    assert_eq!(scene_times(told, 40), vec![10.0, 22.0]);
}

#[test]
fn a_chaotic_video_is_thinned_across_its_whole_length_not_truncated() {
    // A handheld clip where everything is a scene change. Taking the first
    // forty would read the opening titles in detail and never reach the part
    // where anything happened.
    let told: String = (0..400)
        .map(|i| format!("pts_time:{}\n", i * 3))
        .collect();
    let times = scene_times(&told, 40);
    assert_eq!(times.len(), 40);
    assert!(
        *times.last().unwrap() > 1000.0,
        "the end of the video has to be represented: {:?}",
        times.last()
    );
}

#[test]
fn a_short_video_is_not_thinned_at_all() {
    let told = "pts_time:1.0\npts_time:5.0\n";
    assert_eq!(scene_times(told, 40).len(), 2);
}

// ---------------------------------------------------------------------------
// Not saying the same thing eight times
// ---------------------------------------------------------------------------

#[test]
fn a_cursor_moving_over_a_slide_is_still_the_same_slide() {
    let a = "Quarterly review — revenue up eleven percent across all regions";
    let b = "Quarterly review revenue up eleven percent across all regions";
    assert!(
        same_screen(a, b),
        "scene detection fires on a cursor moving and a call re-encoding; an \
         account that says the same thing eight times is one nobody finishes"
    );
}

#[test]
fn a_genuinely_different_screen_is_a_different_screen() {
    assert!(!same_screen(
        "Quarterly review revenue up eleven percent",
        "Terms and conditions payment due within thirty days"
    ));
}

#[test]
fn two_screens_with_nothing_readable_are_not_two_moments() {
    assert!(same_screen("", "   "));
    assert!(!same_screen("", "something appeared"));
}

// ---------------------------------------------------------------------------
// Reading a timed transcript
// ---------------------------------------------------------------------------

const SRT: &str = "\
1
00:00:00,000 --> 00:00:04,500
Right, let me show you the problem.

2
00:00:12,540 --> 00:00:18,000
You can see it here.

3
00:00:37,500 --> 00:00:41,000
So that's the fix.
";

#[test]
fn a_timed_transcript_is_read_into_words_and_times() {
    let said = read_timed(SRT);
    assert_eq!(said.len(), 3);
    assert_eq!(said[0].at, 0.0);
    assert!((said[1].at - 12.54).abs() < 0.01);
    assert_eq!(said[2].words, "So that's the fix.");
}

#[test]
fn one_bad_cue_does_not_throw_away_the_whole_transcript() {
    let broken = format!("{SRT}\n4\nnot a timestamp at all\nsome words\n");
    assert_eq!(
        read_timed(&broken).len(),
        3,
        "refusing an hour of transcript over one malformed line is the wrong \
         trade"
    );
}

#[test]
fn an_empty_transcript_is_empty_rather_than_a_panic() {
    assert!(read_timed("").is_empty());
    assert!(read_timed("nonsense").is_empty());
}

// ---------------------------------------------------------------------------
// Putting the picture back next to the words
// ---------------------------------------------------------------------------

fn screens() -> Vec<(f32, String)> {
    vec![
        (12.54, "error: cannot borrow as mutable".to_string()),
        (37.5, "fn main() { let mut x = 0; }".to_string()),
    ]
}

#[test]
fn what_was_on_screen_is_paired_with_what_was_being_said() {
    let moments = weave(&read_timed(SRT), &screens());
    let at_error = moments
        .iter()
        .find(|m| m.on_screen.as_deref() == Some("error: cannot borrow as mutable"))
        .expect("the error screen is a moment");
    assert_eq!(
        at_error.said.as_deref(),
        Some("You can see it here."),
        "\"you can see it here\" is worthless without the here"
    );
}

#[test]
fn what_was_said_before_the_first_screen_is_not_dropped() {
    let moments = weave(&read_timed(SRT), &screens());
    assert!(
        moments[0]
            .said
            .as_deref()
            .is_some_and(|s| s.contains("let me show you")),
        "the opening of a video is where people say what it is about"
    );
}

#[test]
fn a_screen_nobody_narrated_still_appears() {
    let spoken = vec![Spoken { at: 0.0, words: "Right.".into() }];
    let moments = weave(&spoken, &screens());
    let silent = moments
        .iter()
        .find(|m| m.at == 37.5)
        .expect("the second screen is still a moment");
    assert!(silent.said.is_none());
    assert!(silent.on_screen.is_some(), "the screen is carrying the meaning");
}

#[test]
fn a_repeated_screen_is_folded_rather_than_repeated() {
    let repeated = vec![
        (10.0, "Quarterly review revenue up eleven percent".to_string()),
        (14.0, "Quarterly review — revenue up eleven percent".to_string()),
        (60.0, "Next steps: hire two engineers".to_string()),
    ];
    let moments = weave(&[], &repeated);
    assert_eq!(moments.len(), 2);
}

#[test]
fn a_video_with_no_sound_and_no_readable_screen_says_so() {
    assert!(retell(&[], false).contains("nothing I could read or hear"));
}

// ---------------------------------------------------------------------------
// The account
// ---------------------------------------------------------------------------

#[test]
fn the_account_is_timestamped_so_you_can_scrub_to_it() {
    let moments = weave(&read_timed(SRT), &screens());
    let told = retell(&moments, false);
    assert!(
        told.contains("[0:12]"),
        "\"at 12.54 seconds\" is a number you have to do arithmetic on: {told}"
    );
    assert!(told.contains("on screen:"));
}

#[test]
fn a_minute_is_a_minute_not_sixty_seconds() {
    let m = Moment { at: 1103.4, said: None, on_screen: None, kept_frame: None };
    assert_eq!(m.stamp(), "18:23");
}

#[test]
fn hitting_the_frame_limit_is_said_rather_than_quietly_truncating() {
    let moments = weave(&read_timed(SRT), &screens());
    let told = retell(&moments, true);
    assert!(
        told.contains("changed more often than I read frames"),
        "an account that silently stops is one you trust to be complete: {told}"
    );
    assert!(!retell(&moments, false).contains("more often"));
}

#[test]
fn silence_over_a_screen_is_stated_rather_than_left_blank() {
    let moments = vec![Moment {
        at: 12.0,
        said: None,
        on_screen: Some("the whole answer".into()),
        kept_frame: None,
    }];
    let told = retell(&moments, false);
    assert!(
        told.contains("nothing said"),
        "\"nobody narrated this bit\" matters most exactly when the screen is \
         carrying the meaning: {told}"
    );
}

// ---------------------------------------------------------------------------
// The settings
// ---------------------------------------------------------------------------

#[test]
fn the_defaults_suit_the_thing_he_will_actually_send() {
    let c = ViewConfig::default();
    assert!(c.enabled);
    // Measured, not guessed: 0.35 found zero scene changes on a real
    // 24-second clip. This now only has to catch genuine cuts — the gaps are
    // filled by the floor below, which is what stops a static video going
    // unwatched.
    assert!(
        c.scene_change <= 0.25,
        "0.35 found nothing at all on real footage"
    );
    assert!(
        c.at_least_every_secs > 0.0 && c.at_least_every_secs <= 10.0,
        "a video whose picture never changes must still be looked at"
    );
    assert!(
        c.most_frames >= 20 && c.most_frames <= 60,
        "a cap rather than a rate: the failure to avoid is a chaotic video \
         turning into an hour of text recognition"
    );
}


// ---------------------------------------------------------------------------
// The hole in "keep the reading, delete the frame"
// ---------------------------------------------------------------------------
//
// Reading only ever finds text. A chart, a photo, a product shot, someone
// pointing at a physical thing — text recognition returns nothing trustworthy
// on any of those, and the frame was then deleted. Held together, the two
// rules were quietly dropping every frame that wasn't words. That is partial
// context arrived at from the other direction.

fn unreadable(at: f32, kept: &str) -> Seen {
    Seen { at, text: String::new(), kept_frame: Some(kept.to_string()) }
}

fn readable(at: f32, text: &str) -> Seen {
    Seen { at, text: text.to_string(), kept_frame: None }
}

#[test]
fn a_screen_with_no_words_on_it_is_still_a_moment() {
    let moments = weave_seen(&[], &[unreadable(12.0, "frames-1/000.jpg")]);
    assert_eq!(moments.len(), 1, "a chart is not nothing");
    assert_eq!(
        moments[0].kept_frame.as_deref(),
        Some("frames-1/000.jpg"),
        "the picture is what is worth keeping when the words are empty"
    );
}

#[test]
fn a_screen_that_read_as_words_does_not_also_keep_a_picture() {
    let moments = weave_seen(&[], &[readable(12.0, "error: cannot borrow")]);
    assert!(
        moments[0].kept_frame.is_none(),
        "where the screen turned into words, the words are the thing worth \
         keeping and the frame is the cost that made screenshots wrong"
    );
}

#[test]
fn two_different_unreadable_screens_are_two_moments() {
    // Both have no words, so a words-only comparison calls them identical and
    // folds a chart and a photo into one.
    let moments = weave_seen(
        &[],
        &[unreadable(10.0, "frames-1/000.jpg"), unreadable(40.0, "frames-1/001.jpg")],
    );
    assert_eq!(moments.len(), 2);
    assert_ne!(moments[0].kept_frame, moments[1].kept_frame);
}

#[test]
fn the_account_says_something_was_there_rather_than_going_quiet() {
    let spoken = vec![Spoken { at: 11.0, words: "Look at the shape of that.".into() }];
    let told = retell(&weave_seen(&spoken, &[unreadable(10.0, "frames-1/000.jpg")]), false);
    assert!(
        told.contains("Look at the shape of that"),
        "the words are still there: {told}"
    );
    assert!(
        told.contains("couldn't read as words") && told.contains("kept the picture"),
        "\"look at the shape of that\" with nothing after it is the account \
         quietly becoming partial: {told}"
    );
}

#[test]
fn an_unnarrated_unreadable_screen_is_not_dropped_either() {
    let told = retell(&weave_seen(&[], &[unreadable(10.0, "frames-1/000.jpg")]), false);
    assert!(told.contains("kept the picture"), "{told}");
    assert!(told.contains("[0:10]"), "and it is still timestamped: {told}");
}

#[test]
fn a_mixed_video_keeps_words_where_there_were_words_and_pictures_where_there_were_not() {
    let screens = vec![
        readable(5.0, "Quarterly review revenue up eleven percent"),
        unreadable(30.0, "frames-1/001.jpg"),
        readable(60.0, "Next steps hire two engineers"),
    ];
    let moments = weave_seen(&[], &screens);
    assert_eq!(moments.len(), 3);
    assert!(moments[0].kept_frame.is_none());
    assert!(moments[1].on_screen.is_none() && moments[1].kept_frame.is_some());
    assert!(moments[2].kept_frame.is_none());
}

#[test]
fn a_thumbnail_is_small_enough_that_keeping_it_is_not_the_cost_that_was_objected_to() {
    let c = ViewConfig::default();
    assert!(
        c.thumbnail_width <= 320,
        "at full size a frame is a third of a megabyte; the width is the whole \
         difference between keeping pictures and hoarding screenshots"
    );
    assert!(c.thumbnail_width >= 120, "too small to recognise anything");
}

#[test]
fn the_old_way_of_weaving_still_works_for_screens_that_are_all_words() {
    // `weave` is `weave_seen` with no pictures. Kept so the text-only path
    // has one implementation rather than two that drift.
    let moments = weave(&read_timed(SRT), &screens());
    assert!(moments.iter().all(|m| m.kept_frame.is_none()));
}


// ---------------------------------------------------------------------------
// Measured against a real video Eric sent
// ---------------------------------------------------------------------------
//
// A 24-second handheld clip of a laptop screen. At the threshold originally
// shipped it produced **zero** scene changes; at 0.05 it produced three, two of
// which were 40 milliseconds apart. Scene detection assumes the picture changes
// when the content does — true of a screen recording, false of a phone pointed
// at something, a talking head, a slow pan, or captions over a static shot.
//
// A video whose picture does not change is not a video with nothing in it.

#[test]
fn a_video_with_no_scene_changes_is_still_looked_at() {
    let cfg = ViewConfig::default();
    let looks = where_to_look(&[], 24.47, &cfg);
    assert!(
        looks.len() >= 5,
        "the real clip this was measured on would have been watched with zero \
         frames: {looks:?}"
    );
    assert_eq!(looks[0], 0.0, "always look at the start");
    assert!(
        *looks.last().unwrap() > 18.0,
        "and keep looking to the end: {looks:?}"
    );
}

#[test]
fn no_gap_is_longer_than_the_floor() {
    let cfg = ViewConfig::default();
    let looks = where_to_look(&[2.0, 3.0], 60.0, &cfg);
    for pair in looks.windows(2) {
        assert!(
            pair[1] - pair[0] <= cfg.at_least_every_secs + 0.6,
            "went {} seconds without looking: {looks:?}",
            pair[1] - pair[0]
        );
    }
}

#[test]
fn a_busy_video_is_led_by_its_cuts_not_by_the_clock() {
    let cfg = ViewConfig::default();
    let cuts: Vec<f32> = (0..20).map(|i| i as f32 * 1.5).collect();
    let looks = where_to_look(&cuts, 30.0, &cfg);
    for c in &cuts {
        assert!(
            looks.iter().any(|l| (l - c).abs() < 0.5),
            "cut at {c} was not looked at: {looks:?}"
        );
    }
}

#[test]
fn a_video_static_then_busy_gets_both_treatments() {
    let cfg = ViewConfig::default();
    // Nothing happens for a minute, then rapid cuts.
    let cuts = vec![60.0, 61.0, 62.0, 63.0];
    let looks = where_to_look(&cuts, 65.0, &cfg);
    assert!(
        looks.iter().filter(|l| **l < 60.0).count() >= 5,
        "the quiet minute must not be blank: {looks:?}"
    );
    assert!(
        looks.iter().filter(|l| **l >= 60.0).count() >= 4,
        "and every cut still gets looked at: {looks:?}"
    );
}

#[test]
fn the_cap_still_holds_and_still_spans_the_whole_video() {
    let cfg = ViewConfig::default();
    let looks = where_to_look(&[], 3600.0, &cfg);
    assert!(looks.len() <= cfg.most_frames);
    assert!(
        *looks.last().unwrap() > 3000.0,
        "an hour-long video must not be covered in detail for its first two \
         minutes and then abandoned: {:?}",
        looks.last()
    );
}

#[test]
fn an_unknown_duration_asks_for_fewer_frames_rather_than_runaway() {
    let looks = where_to_look(&[], 0.0, &ViewConfig::default());
    assert!(looks.len() <= 1, "cautious direction: {looks:?}");
}

// ---------------------------------------------------------------------------
// One frame per moment
// ---------------------------------------------------------------------------

#[test]
fn every_frame_ffmpeg_reported_is_read_including_the_neighbours() {
    // `scene_times` collapses near-duplicates, which is right for deciding
    // where to look and wrong for counting what came back.
    let told = "pts_time:4.00
pts_time:4.03
pts_time:8.00
";
    assert_eq!(frame_times(told), vec![4.0, 4.03, 8.0]);
    assert_eq!(scene_times(told, 40).len(), 2, "these differ on purpose");
}

#[test]
fn two_frames_a_thirtieth_of_a_second_apart_are_one_moment() {
    let frames: Vec<std::path::PathBuf> =
        ["a.png", "b.png", "c.png"].iter().map(Into::into).collect();
    let (keep, (times, drop)) = one_per_moment(frames, &[4.0, 4.03, 8.0], &[4.0, 8.0]);
    assert_eq!(keep.len(), 2, "six requested moments produced twelve frames");
    assert_eq!(times, vec![4.0, 8.0]);
    assert_eq!(drop.len(), 1, "and the extra is handed back to be deleted");
    assert_eq!(drop[0], std::path::PathBuf::from("b.png"));
}

#[test]
fn a_frame_with_no_reported_time_is_dropped_not_placed_at_the_start() {
    let frames: Vec<std::path::PathBuf> = ["a.png", "b.png"].iter().map(Into::into).collect();
    let (keep, (times, drop)) = one_per_moment(frames, &[], &[]);
    assert!(
        keep.is_empty() && times.is_empty(),
        "a missing timestamp becoming 0.0 would put every unlabelled frame at \
         the start of the video"
    );
    assert_eq!(drop.len(), 2, "and nothing is left behind on disk");
}

#[test]
fn nothing_is_kept_that_is_not_also_tracked() {
    let frames: Vec<std::path::PathBuf> =
        ["a.png", "b.png", "c.png", "d.png"].iter().map(Into::into).collect();
    let (keep, (times, drop)) = one_per_moment(frames, &[0.0, 0.02, 4.0, 4.01], &[]);
    assert_eq!(keep.len(), times.len(), "a frame with no time is untracked");
    assert_eq!(keep.len() + drop.len(), 4, "every file is accounted for");
}
