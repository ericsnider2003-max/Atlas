use atlas::browser::{BrowserConfig, PostStep};
use atlas::delivery::{classify, plan, profile_for, spoken, Outcome};
use atlas::edit::{
    atempo_chain, describe, duration_from_probe, escape_drawtext, ffmpeg_args, plan_from_model,
    probe_args, EditPlan, Overlay, Segment,
};
use atlas::error::AtlasError;
use atlas::publish::{Channel, Publisher};

// ================= approved post -> browser =================

fn ready_post(p: &mut Publisher, ch: Channel, body: &str) -> u64 {
    let id = p.draft(ch, body);
    p.approve(id);
    id
}

#[test]
fn an_approved_post_becomes_a_sequence_ending_in_submit() {
    let cfg = BrowserConfig::default();
    let mut pub_ = Publisher::default();
    let id = ready_post(&mut pub_, Channel::X, "shipping today");
    let steps = plan(&pub_, &cfg, id, true).unwrap();
    assert_eq!(steps.last(), Some(&PostStep::Submit));
    assert!(steps.iter().any(|s| matches!(s, PostStep::Fill(t) if t == "shipping today")));
}

#[test]
fn an_unapproved_post_never_produces_a_plan_at_all() {
    let cfg = BrowserConfig::default();
    let mut pub_ = Publisher::default();
    let id = pub_.draft(Channel::X, "not approved");
    match plan(&pub_, &cfg, id, true) {
        Err(Outcome::Blocked(_)) => {}
        o => panic!("expected blocked, got {o:?}"),
    }
}

#[test]
fn a_post_edited_after_approval_is_blocked_at_the_delivery_layer_too() {
    // Defence in depth: the publisher checks, and so does this.
    let cfg = BrowserConfig::default();
    let mut pub_ = Publisher::default();
    let id = ready_post(&mut pub_, Channel::X, "original");
    pub_.edit(id, "swapped");
    match plan(&pub_, &cfg, id, true) {
        Err(Outcome::Blocked(why)) => assert!(why.contains("changed since you approved")),
        o => panic!("{o:?}"),
    }
}

#[test]
fn being_offline_is_a_retry_not_a_failure() {
    let cfg = BrowserConfig::default();
    let mut pub_ = Publisher::default();
    let id = ready_post(&mut pub_, Channel::X, "hello");
    match plan(&pub_, &cfg, id, false) {
        Err(Outcome::Retry(why)) => assert!(why.contains("connection")),
        o => panic!("{o:?}"),
    }
}

#[test]
fn a_channel_with_no_way_to_post_says_so_instead_of_pretending() {
    let cfg = BrowserConfig::default();
    let mut pub_ = Publisher::default();
    let id = ready_post(
        &mut pub_,
        Channel::Email { to: "a@b.com".into(), subject: "hi".into() },
        "body",
    );
    match plan(&pub_, &cfg, id, true) {
        Err(Outcome::Blocked(why)) => assert!(why.contains("no way to post")),
        o => panic!("{o:?}"),
    }
}

#[test]
fn channels_map_to_the_right_site_profile() {
    let cfg = BrowserConfig::default();
    assert_eq!(profile_for(&cfg, &Channel::X).unwrap().name, "x");
    assert_eq!(profile_for(&cfg, &Channel::LinkedIn).unwrap().name, "linkedin");
    assert!(profile_for(&cfg, &Channel::Email { to: "a".into(), subject: "b".into() }).is_none());
}

#[test]
fn a_slow_page_is_retried_but_a_missing_login_is_not() {
    // Retrying a permanent failure forever is noise; giving up on a slow page
    // loses a post you approved.
    assert!(matches!(
        classify(AtlasError::Platform("Page.navigate timed out".into())),
        Outcome::Retry(_)
    ));
    assert!(matches!(
        classify(AtlasError::Platform("x never finished loading".into())),
        Outcome::Retry(_)
    ));
    assert!(matches!(
        classify(AtlasError::Platform("not signed in to x — sign in and try again".into())),
        Outcome::Blocked(_)
    ));
}

#[test]
fn a_changed_site_layout_is_reported_as_such() {
    let o = classify(AtlasError::Platform(
        "none of these are on the page: div[x], div[y]".into(),
    ));
    match o {
        Outcome::Blocked(why) => assert!(why.contains("layout may have changed"), "got: {why}"),
        o => panic!("{o:?}"),
    }
}

#[test]
fn what_atlas_says_afterwards_is_one_short_line() {
    for o in [
        Outcome::Sent("posted to X".into()),
        Outcome::Retry("timeout".into()),
        Outcome::Blocked("not signed in".into()),
    ] {
        let s = spoken(&o);
        assert!(s.len() < 70 && !s.contains('\n'), "got: {s}");
    }
}

// ================= video editing =================

fn three_cuts() -> EditPlan {
    EditPlan {
        sources: vec!["raw/take1.mp4".into()],
        segments: vec![
            Segment { source: 0, start: 12.0, end: 30.0, speed: 1.0 },
            Segment { source: 0, start: 45.0, end: 60.0, speed: 1.0 },
            Segment { source: 0, start: 2.0, end: 8.0, speed: 1.0 },
        ],
        overlays: vec![],
        music: None,
        music_gain_db: -18.0,
        output: "out/edited.mp4".into(),
        resolution: None,
        fps: None,
        intent: "cut the dead air, best bit first".into(),
    }
}

#[test]
fn a_plan_that_would_overwrite_the_source_is_refused() {
    // The one check that actually protects your footage.
    let mut p = three_cuts();
    p.output = "raw/take1.mp4".into();
    let e = p.validate(&[120.0]).unwrap_err().to_string();
    assert!(e.contains("overwrite a source"), "got: {e}");
}

#[test]
fn path_separators_and_case_do_not_defeat_the_overwrite_check() {
    let mut p = three_cuts();
    p.sources = vec![r"C:\Raw\Take1.mp4".into()];
    p.output = "c:/raw/take1.mp4".into();
    assert!(p.validate(&[120.0]).is_err());
}

#[test]
fn a_segment_running_past_the_end_of_the_video_is_caught_before_ffmpeg() {
    let p = three_cuts();
    let e = p.validate(&[40.0]).unwrap_err().to_string();
    assert!(e.contains("past the end"), "got: {e}");
}

#[test]
fn backwards_and_nonsense_timestamps_are_caught() {
    let mut p = three_cuts();
    p.segments[0] = Segment { source: 0, start: 30.0, end: 12.0, speed: 1.0 };
    assert!(p.validate(&[120.0]).unwrap_err().to_string().contains("ends before it starts"));

    let mut p2 = three_cuts();
    p2.segments[0].start = f64::NAN;
    assert!(p2.validate(&[120.0]).is_err());
}

#[test]
fn a_segment_pointing_at_a_video_that_is_not_there_is_caught() {
    let mut p = three_cuts();
    p.segments[1].source = 7;
    assert!(p.validate(&[120.0]).unwrap_err().to_string().contains("isn't there"));
}

#[test]
fn an_absurd_speed_is_refused() {
    let mut p = three_cuts();
    p.segments[0].speed = 50.0;
    assert!(p.validate(&[120.0]).unwrap_err().to_string().contains("out of range"));
}

#[test]
fn an_empty_plan_is_refused_rather_than_rendering_nothing() {
    let mut p = three_cuts();
    p.segments.clear();
    assert!(p.validate(&[120.0]).unwrap_err().to_string().contains("no segments"));
}

#[test]
fn a_caption_after_the_end_of_the_edit_is_caught() {
    let mut p = three_cuts();
    p.overlays.push(Overlay {
        text: "the end".into(),
        start: 500.0,
        end: 505.0,
        position: "bottom".into(),
        size: 36,
    });
    assert!(p.validate(&[120.0]).unwrap_err().to_string().contains("after the video ends"));
}

#[test]
fn a_sound_plan_validates() {
    assert!(three_cuts().validate(&[120.0]).is_ok());
}

#[test]
fn duration_is_the_sum_of_the_kept_segments() {
    assert_eq!(three_cuts().duration(), 18.0 + 15.0 + 6.0);
}

#[test]
fn speeding_a_segment_up_shortens_it() {
    let mut p = three_cuts();
    p.segments[0].speed = 2.0;
    assert_eq!(p.segments[0].duration(), 9.0);
}

#[test]
fn every_segment_is_trimmed_and_pts_reset_then_concatenated() {
    // Stream copying cannot cut on arbitrary frames; this is why.
    let args = ffmpeg_args(&three_cuts());
    let f = args.iter().find(|a| a.contains("concat=")).expect("a filter graph");
    assert!(f.contains("trim=start=12.000:end=30.000"));
    assert!(f.contains("setpts=(PTS-STARTPTS)"));
    assert!(f.contains("concat=n=3:v=1:a=1"));
}

#[test]
fn the_render_refuses_to_clobber_an_existing_output() {
    let args = ffmpeg_args(&three_cuts());
    assert!(args.contains(&"-n".to_string()), "must not pass -y");
    assert!(!args.contains(&"-y".to_string()));
    assert_eq!(args.last(), Some(&"out/edited.mp4".to_string()));
}

#[test]
fn large_speed_changes_chain_atempo_because_it_caps_at_two() {
    assert_eq!(atempo_chain(1.0), "");
    assert!(atempo_chain(2.0).contains("atempo=2.000"));
    let fast = atempo_chain(4.0);
    assert_eq!(fast.matches("atempo").count(), 2, "got: {fast}");
    let slow = atempo_chain(0.25);
    assert_eq!(slow.matches("atempo").count(), 2, "got: {slow}");
}

#[test]
fn an_apostrophe_in_a_caption_cannot_break_the_filter_string() {
    let e = escape_drawtext("Here's the thing: 50% done");
    assert!(!e.contains("Here's"), "the raw apostrophe must not survive: {e}");
    assert!(e.contains(r"\\\'"), "apostrophe escaped for drawtext: {e}");
    assert!(e.contains("\\:"), "colons must be escaped: {e}");
    assert!(e.contains("\\%"), "percent must be escaped: {e}");
}

#[test]
fn captions_appear_only_between_their_timestamps() {
    let mut p = three_cuts();
    p.overlays.push(Overlay {
        text: "Part one".into(),
        start: 0.0,
        end: 3.0,
        position: "top".into(),
        size: 48,
    });
    let f = ffmpeg_args(&p).join(" ");
    assert!(f.contains("drawtext"));
    assert!(f.contains("enable='between(t,0.000,3.000)'"), "got: {f}");
    assert!(f.contains("fontsize=48"));
}

#[test]
fn background_music_is_mixed_under_rather_than_replacing_the_audio() {
    let mut p = three_cuts();
    p.music = Some("music/bed.mp3".into());
    let f = ffmpeg_args(&p).join(" ");
    assert!(f.contains("amix=inputs=2"), "original audio must survive");
    assert!(f.contains("volume=-18.0dB"), "and the bed must sit under it");
}

#[test]
fn a_resolution_change_is_applied_before_captions_are_drawn() {
    let mut p = three_cuts();
    p.resolution = Some((1080, 1920));
    p.overlays.push(Overlay {
        text: "hi".into(), start: 0.0, end: 1.0, position: "bottom".into(), size: 36,
    });
    let f = ffmpeg_args(&p).join(" ");
    let scale_at = f.find("scale=1080:1920").expect("a scale filter");
    let text_at = f.find("drawtext").expect("a text filter");
    assert!(scale_at < text_at, "captions must be sized to the final frame");
}

#[test]
fn a_model_reply_becomes_a_plan_even_wrapped_in_prose() {
    let reply = "Sure! ```json\n{\"segments\":[{\"source\":0,\"start\":1.0,\"end\":9.0}],\
                 \"intent\":\"tighten the open\"}\n``` hope that helps";
    let p = plan_from_model(reply, vec!["raw/a.mp4".into()], "out/b.mp4").unwrap();
    assert_eq!(p.segments.len(), 1);
    assert_eq!(p.segments[0].speed, 1.0, "speed defaults when the model omits it");
    assert_eq!(p.intent, "tighten the open");
    assert_eq!(p.output, "out/b.mp4");
}

#[test]
fn a_planner_reply_that_is_not_json_is_a_clear_error() {
    assert!(plan_from_model("I can't do that", vec!["a.mp4".into()], "b.mp4").is_err());
}

#[test]
fn a_planner_that_invents_an_impossible_edit_is_caught_by_validation() {
    let reply = r#"{"segments":[{"source":0,"start":0,"end":9999}]}"#;
    let p = plan_from_model(reply, vec!["raw/a.mp4".into()], "out/b.mp4").unwrap();
    assert!(p.validate(&[60.0]).is_err(), "the model's arithmetic is not trusted");
}

#[test]
fn probing_asks_for_duration_and_streams_as_json() {
    let a = probe_args("raw/take1.mp4");
    assert!(a.contains(&"-of".to_string()) && a.contains(&"json".to_string()));
    assert_eq!(a.last(), Some(&"raw/take1.mp4".to_string()));
}

#[test]
fn duration_is_read_back_from_the_probe() {
    let json = r#"{"format":{"duration":"123.456"},"streams":[]}"#;
    assert_eq!(duration_from_probe(json), Some(123.456));
    assert_eq!(duration_from_probe("{}"), None);
    assert_eq!(duration_from_probe("not json"), None);
}

#[test]
fn the_result_is_described_in_one_spoken_line() {
    let d = describe(&three_cuts(), 120.0);
    assert!(d.contains("3 cuts"), "got: {d}");
    assert!(d.contains("120s down to 39s"), "got: {d}");
    assert!(!d.contains('\n'));
}
