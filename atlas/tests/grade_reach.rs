use atlas::editors::{best_for, used, where_to_look, without_anything, Editor, EditorConfig, Job};
use atlas::grade::{
    audio_chain, check_audio, check_picture, preset_filter, presets,
    recording_advice, GradeConfig,
    spoken as grade_spoken, Audio, Picture, SafeArea, MAX_TRUE_PEAK_DB, TARGET_LUFS,
};
use atlas::judgment::JudgmentConfig;
use atlas::reach::{
    direction, findings, outlier, quality_against, spoken as reach_spoken, Direction, Post,
    ReachConfig, Sure, ENOUGH_POSTS, WHAT_MAKES_A_POST_GOOD,
};

/// `judgment`'s default floor is trading's thirty. `reach` narrows it to its
/// own, and so must anything testing `reach`.
fn judging() -> JudgmentConfig {
    JudgmentConfig { min_seen: ENOUGH_POSTS, ..JudgmentConfig::default() }
}

fn quiet_audio() -> Audio {
    Audio { lufs: -22.0, true_peak_db: -6.0, range_db: 9.0, noise_floor_db: -60.0,
            harsh_s: false, rumble: false }
}

fn good_audio() -> Audio {
    Audio { lufs: -14.2, true_peak_db: -1.4, range_db: 8.0, noise_floor_db: -62.0,
            harsh_s: false, rumble: false }
}

fn crushed() -> Picture {
    Picture { clipped_black: 0.09, clipped_white: 0.002, brightness: 0.42,
              skin_kelvin: Some(5400), saturation: 1.1, width: 1080, height: 1920, fps: 30.0 }
}

fn good_picture() -> Picture {
    Picture { clipped_black: 0.004, clipped_white: 0.001, brightness: 0.5,
              skin_kelvin: Some(5600), saturation: 1.08, width: 1080, height: 1920, fps: 30.0 }
}

// ================= it's a number, not a taste =================

#[test]
fn being_quiet_is_explained_as_what_the_platform_does_to_you() {
    // Delivering quieter doesn't make you safe, it makes you quiet.
    let n = check_audio(&quiet_audio(), &GradeConfig::default());
    assert_eq!(n[0].what, "it's quiet");
    assert!(n[0].because.contains("-22.0 LUFS"));
    assert!(n[0].because.contains("everything around you gets raised"));
    assert!(n[0].fix.contains("loudnorm"));
}

#[test]
fn every_note_carries_the_measurement_behind_it() {
    for n in check_audio(&quiet_audio(), &GradeConfig::default()).iter().chain(check_picture(&crushed()).iter()) {
        assert!(!n.because.is_empty(), "\"{}\" has no number behind it", n.what);
        assert!(!n.fix.is_empty());
    }
}

#[test]
fn crushed_blacks_are_the_first_thing_said_about_a_picture() {
    // The most common thing that makes footage look cheap, and people do it
    // to themselves reaching for contrast.
    let n = check_picture(&crushed());
    assert_eq!(n[0].what, "the shadows are crushed");
    assert!(n[0].fix.contains("Contrast comes from the curve, not the floor"));
}

#[test]
fn skin_tone_off_is_named_as_orange_or_blue_rather_than_in_kelvin() {
    let orange = Picture { skin_kelvin: Some(3800), ..good_picture() };
    let n = check_picture(&orange);
    assert!(n.iter().any(|n| n.what.contains("orange")));

    let blue = Picture { skin_kelvin: Some(7800), ..good_picture() };
    assert!(check_picture(&blue).iter().any(|n| n.what.contains("blue")));
}

#[test]
fn letterboxing_is_called_out_because_bars_read_as_reposted() {
    let landscape = Picture { width: 1920, height: 1080, ..good_picture() };
    let n = check_picture(&landscape);
    assert!(n.iter().any(|n| n.fix.contains("bars read as reposted")));
}

#[test]
fn something_that_needs_re_recording_is_told_apart_from_something_fixable() {
    // Fixing it in the edit every time is a tax you pay forever.
    let blown = Picture { clipped_white: 0.12, ..good_picture() };
    let n = check_picture(&blown);
    let highlight = n.iter().find(|n| n.what.contains("highlights")).unwrap();
    assert!(!highlight.fixable_now);

    let low_fps = Picture { fps: 24.0, ..good_picture() };
    let advice = recording_advice(&check_picture(&low_fps));
    assert!(!advice.is_empty(), "and it becomes advice about next time");
}

#[test]
fn good_material_is_left_alone() {
    let said = grade_spoken(&check_audio(&good_audio(), &GradeConfig::default()), &check_picture(&good_picture()));
    assert!(said.contains("Nothing worth changing"));
}

#[test]
fn the_audio_chain_normalises_last_because_anything_after_undoes_it() {
    let noisy = Audio { rumble: true, harsh_s: true, range_db: 18.0, noise_floor_db: -40.0,
                        ..quiet_audio() };
    let chain = audio_chain(&noisy);
    assert!(chain.last().unwrap().contains("loudnorm"));
    assert!(chain[0].contains("highpass"), "rumble goes first");
    assert!(chain.iter().any(|c| c.contains("acompressor")));
    assert!(chain.last().unwrap().contains(&format!("I={TARGET_LUFS}")));
    assert!(chain.last().unwrap().contains(&format!("TP={MAX_TRUE_PEAK_DB}")));
}

#[test]
fn clean_audio_gets_only_the_normalising() {
    assert_eq!(audio_chain(&good_audio()).len(), 1);
}

// ================= where the interface covers your frame =================

#[test]
fn text_under_the_interface_is_text_nobody_reads() {
    let s = SafeArea::typical();
    assert!(!s.clears(50.0, 90.0), "bottom is covered by the caption and buttons");
    assert!(!s.clears(92.0, 50.0), "right side is the button column");
    assert!(s.clears(40.0, 60.0));
}

#[test]
fn there_is_a_band_captions_should_sit_in() {
    let (top, bottom) = SafeArea::typical().caption_band();
    assert!(top < bottom);
    assert!(SafeArea::typical().clears(50.0, (top + bottom) / 2.0));
}

// ================= presets you can start from =================

#[test]
fn the_presets_are_few_and_restrained() {
    // The ones that look like a filter are the ones that date.
    let p = presets();
    assert!(p.len() <= 5);
    assert!(p.iter().all(|p| p.saturation <= 1.1), "nothing oversaturated");
    assert!(p.iter().all(|p| !p.what_it_is.is_empty() && !p.for_what.is_empty()));
}

#[test]
fn every_preset_is_a_filter_ffmpeg_can_apply() {
    for p in presets() {
        let f = preset_filter(&p);
        assert!(f.contains("eq=brightness"));
        assert!(f.contains("colortemperature"));
    }
}

// ================= one post is not a finding =================

fn post(id: &str, at: u64, views: u64, held: f32, completion: f32, hook: &str, topic: &str, secs: f32) -> Post {
    Post {
        id: id.into(), at, views, held_at_three: held, completion,
        saves: (views / 50).max(1), shares: (views / 100).max(1), comments: 5, follows: views / 500,
        topic: topic.into(), hook: hook.into(), seconds: secs,
    }
}

#[test]
fn one_post_is_never_reported_as_a_finding() {
    // The single most expensive mistake in content.
    let one = vec![post("a", 0, 90_000, 0.8, 0.6, "contradiction", "fees", 24.0)];
    assert!(findings(&one, &judging()).is_empty());
    assert!(reach_spoken(&one, &ReachConfig::default(), &judging()).contains("not enough to tell you anything I'd trust"));
}

#[test]
fn two_posts_agreeing_is_worth_watching_not_acting_on() {
    assert_eq!(Sure::from_count(2, true), Sure::Maybe);
    assert!(!Sure::Maybe.worth_acting_on());
    assert!(Sure::Fairly.worth_acting_on());
}

#[test]
fn a_group_carried_by_one_outlier_is_not_called_consistent() {
    // Otherwise one lucky post makes a whole category look good.
    assert_eq!(Sure::from_count(5, false), Sure::Maybe);
    assert_eq!(Sure::from_count(5, true), Sure::Fairly);
}

#[test]
fn a_pattern_across_several_posts_is_reported_with_how_sure_it_is() {
    let mut posts = Vec::new();
    for i in 0..5 {
        posts.push(post(&format!("c{i}"), i * 1000, 5000, 0.78, 0.5, "contradiction", "fees", 24.0));
    }
    for i in 0..5 {
        posts.push(post(&format!("q{i}"), 5000 + i * 1000, 5000, 0.3, 0.12, "question", "general", 55.0));
    }
    let f = findings(&posts, &judging());
    let best = f.iter().find(|f| f.what.contains("contradiction")).expect("should find it");
    assert!(best.sure.worth_acting_on());
    assert!(best.lift > 1.25);
    assert!(best.across >= 4);
}

#[test]
fn direction_is_a_trend_not_a_comparison_with_the_last_post() {
    let mut posts = Vec::new();
    for i in 0..5 {
        posts.push(post(&format!("old{i}"), i * 1000, 2000, 0.35, 0.15, "question", "x", 50.0));
    }
    for i in 0..5 {
        posts.push(post(&format!("new{i}"), 10_000 + i * 1000, 2000, 0.75, 0.5, "contradiction", "x", 24.0));
    }
    let (d, why) = direction(&posts, &ReachConfig::default(), &judging());
    assert_eq!(d, Direction::Up);
    assert!(why.contains("recent half"));
}

#[test]
fn too_few_posts_gives_no_direction_rather_than_a_guess() {
    let few = vec![post("a", 0, 100, 0.5, 0.3, "x", "y", 20.0)];
    assert_eq!(direction(&few, &ReachConfig::default(), &judging()).0, Direction::Unclear);
}

/// Nine ordinary posts, so there is a distribution to be unusual against.
///
/// Was five. `outlier` used to need four posts and `typical * 5.0`, and on
/// four posts a median of two and a best of eleven clears that — which view
/// counts do constantly, being heavy-tailed. The floor is `Sure::Confident`'s
/// own eight now: telling somebody one of their posts is exceptional invites
/// them to copy it, and that needs more than a "worth watching" sample.
fn ordinary_run() -> Vec<Post> {
    (0..9)
        .map(|i| post(&format!("n{i}"), i * 1000, 2000, 0.6, 0.4, "contradiction", "fees", 24.0))
        .collect()
}

#[test]
fn a_viral_post_that_did_not_hold_is_named_as_exactly_that() {
    // It got shown around. It didn't land. Building on it wastes a month.
    let mut posts = ordinary_run();
    posts.push(post("viral", 9000, 400_000, 0.28, 0.06, "question", "random", 55.0));

    let (p, note) = outlier(&posts, &judging()).expect("should spot it");
    assert_eq!(p.id, "viral");
    assert!(note.contains("didn't hold"), "{note}");
    assert!(note.contains("Don't build on it"), "{note}");
}

#[test]
fn a_viral_post_that_did_hold_is_still_only_one_post() {
    let mut posts = ordinary_run();
    posts.push(post("viral", 9000, 400_000, 0.81, 0.55, "contradiction", "fees", 22.0));
    let (_, note) = outlier(&posts, &judging()).unwrap();
    assert!(note.contains("still one post"), "{note}");
}

// ================= the two numbers from nowhere =================

#[test]
fn a_lucky_post_and_a_good_post_are_told_apart() {
    // The distinction the view count alone could never make. Both got far
    // more views than usual; one was a better post underneath and one was
    // not, and "5x your usual views" says the same thing about both.
    let mut lucky = ordinary_run();
    lucky.push(post("lucky", 9000, 400_000, 0.22, 0.05, "question", "random", 55.0));
    let (_, said) = outlier(&lucky, &judging()).expect("spotted");
    assert!(said.contains("weaker post than you usually make"), "{said}");

    let mut good = ordinary_run();
    good.push(post("good", 9000, 400_000, 0.88, 0.72, "contradiction", "fees", 22.0));
    let (_, said) = outlier(&good, &judging()).expect("spotted");
    assert!(said.contains("better post than you usually make"), "{said}");
}

#[test]
fn whether_it_held_is_measured_against_your_own_posts() {
    // `held_at_three > 0.6` was a line from nowhere. A creator whose posts
    // usually hold 0.35 has a very good one at 0.55; one whose posts usually
    // hold 0.75 has a poor one at 0.62. A fixed line calls the first a
    // failure and the second a success.
    let mut low: Vec<Post> = (0..9)
        .map(|i| post(&format!("l{i}"), i * 1000, 2000, 0.35, 0.3, "contradiction", "fees", 24.0))
        .collect();
    low.push(post("theirs", 9000, 400_000, 0.55, 0.45, "contradiction", "fees", 22.0));
    let (_, said) = outlier(&low, &judging()).expect("spotted");
    assert!(said.contains("and it held"), "0.55 is good for this person: {said}");

    let mut high: Vec<Post> = (0..9)
        .map(|i| post(&format!("h{i}"), i * 1000, 2000, 0.75, 0.6, "contradiction", "fees", 24.0))
        .collect();
    high.push(post("theirs", 9000, 400_000, 0.62, 0.5, "contradiction", "fees", 22.0));
    let (_, said) = outlier(&high, &judging()).expect("spotted");
    assert!(said.contains("didn't hold"), "0.62 is poor for this person: {said}");
}

#[test]
fn a_huge_multiple_of_a_small_median_is_not_an_outlier_if_several_did_well() {
    // What `typical * 5.0` fired on, and the case that took a fixture or two
    // to find.
    //
    // Four quiet posts and four that did well. The median sits down among the
    // quiet ones, so the best post is eighty times it — the old rule fires
    // hard. But four others are in the same range, so the best is **not**
    // exceptional for this run, and that is what the question was actually
    // asking.
    let views = [10u64, 10, 10, 10, 10, 500, 600, 700, 800];
    let run: Vec<Post> = views
        .iter()
        .enumerate()
        .map(|(i, v)| post(&format!("q{i}"), i as u64 * 1000, *v, 0.6, 0.4, "contradiction", "fees", 24.0))
        .collect();

    let multiple = 800.0 / 10.0;
    assert!(multiple > 5.0, "the fixture must clear the old rule: {multiple}x");
    assert!(
        outlier(&run, &judging()).is_none(),
        "it called the best of four good posts an outlier"
    );

    // And the control: the same run with one post genuinely far out.
    let mut real = run.clone();
    real.push(post("far", 9000, 40_000, 0.6, 0.4, "contradiction", "fees", 24.0));
    assert!(outlier(&real, &judging()).is_some(), "it missed a real one");
}

#[test]
fn too_few_posts_to_have_a_distribution_says_nothing() {
    let four: Vec<Post> = (0..3)
        .map(|i| post(&format!("f{i}"), i * 1000, 2000, 0.6, 0.4, "contradiction", "fees", 24.0))
        .chain(std::iter::once(post("viral", 9000, 400_000, 0.9, 0.7, "x", "y", 22.0)))
        .collect();
    assert!(
        outlier(&four, &judging()).is_none(),
        "it called an outlier on four posts"
    );
}

// ================= the composite that had a magic twenty in it =========

#[test]
fn how_good_a_post_was_is_measured_against_your_own_posts() {
    // `Post::quality` was `held*0.5 + completion*0.3 + kept()*20.0*0.2`. Two
    // of those are fractions and `kept()` is a rate around 0.02, so the
    // twenty was there to bring it into range — and it only works if the real
    // rate happens to sit near 0.05. If it is 0.005 the "0.2 weight" was
    // really 0.02.
    let posts = ordinary_run();
    let better = post("better", 9000, 2000, 0.85, 0.7, "contradiction", "fees", 22.0);
    let worse = post("worse", 9000, 2000, 0.3, 0.15, "contradiction", "fees", 22.0);

    let mut with_better = posts.clone();
    with_better.push(better.clone());
    let (b, _) = quality_against(&better, &with_better, &judging());

    let mut with_worse = posts.clone();
    with_worse.push(worse.clone());
    let (w, _) = quality_against(&worse, &with_worse, &judging());

    let (b, w) = (b.expect("measured"), w.expect("measured"));
    assert!(b > 0.0, "a better post scored at or below ordinary: {b}");
    assert!(w < 0.0, "a worse post scored at or above ordinary: {w}");
}

#[test]
fn a_part_with_nothing_behind_it_is_named_rather_than_counted_as_ordinary() {
    // Every post in this run has the same save rate, so `kept()` has no
    // spread and nothing can be unusual about it. That is a part Atlas could
    // not measure, and a score built on two of three is a different claim
    // from one built on three.
    let flat: Vec<Post> = (0..9)
        .map(|i| Post {
            saves: 10,
            shares: 5,
            follows: 1,
            views: 2000,
            ..post(&format!("k{i}"), i * 1000, 2000, 0.4 + i as f32 * 0.03, 0.3, "c", "f", 24.0)
        })
        .collect();
    let (_score, thin) = quality_against(&flat[0], &flat, &judging());
    assert!(thin.contains(&"worth keeping"), "{thin:?}");
}

#[test]
fn the_weights_are_weights_and_nothing_else() {
    // The point of separating them: they are opinion, and they can be argued
    // with or changed without re-measuring anything. No scaling constant
    // hides among them.
    let total: f64 = WHAT_MAKES_A_POST_GOOD.iter().map(|(_, w)| w).sum();
    assert!((total - 1.0).abs() < 1e-9, "the weights no longer add to one: {total}");
    for (name, w) in WHAT_MAKES_A_POST_GOOD {
        assert!(*w > 0.0 && *w < 1.0, "{name} carries a scaling factor, not a weight: {w}");
        assert!(name.len() > 5, "{name:?} is not something to say out loud");
    }
    // Held-at-three counts most, because nothing downstream happens without
    // it. Asserted so that reordering them has to be deliberate.
    assert_eq!(WHAT_MAKES_A_POST_GOOD[0].0, "held at three seconds");
    assert!(WHAT_MAKES_A_POST_GOOD[0].1 > WHAT_MAKES_A_POST_GOOD[1].1);
}

#[test]
fn views_alone_do_not_make_a_post_good() {
    let loud = post("a", 0, 500_000, 0.25, 0.05, "x", "y", 60.0);
    let quiet = post("b", 0, 2_000, 0.8, 0.6, "x", "y", 22.0);
    assert!(quiet.quality() > loud.quality());
}

// ================= software you may or may not own =================

#[test]
fn everything_basic_works_with_nothing_installed() {
    // A system whose basic functions need a subscription isn't one you can
    // hand to anyone.
    let jobs = without_anything();
    assert!(jobs.len() >= 6);
    assert!(jobs.contains(&Job::Captions));
    assert!(jobs.contains(&Job::Loudness));
    assert!(jobs.contains(&Job::ColourCorrect));
}

#[test]
fn with_nothing_installed_atlas_still_picks_ffmpeg_and_says_what_it_cannot_do() {
    let (tool, why) = best_for(Job::ColourGrade, &[]);
    assert_eq!(tool, Editor::Ffmpeg);
    assert!(why.contains("can correct but not really grade"));
    assert!(why.contains("free"), "and names the free option: {why}");
}

#[test]
fn with_resolve_installed_grading_goes_there() {
    let (tool, why) = best_for(Job::ColourGrade, &[Editor::Resolve]);
    assert_eq!(tool, Editor::Resolve);
    assert!(why.contains("free"));
}

#[test]
fn premiere_is_used_for_handing_you_something_to_finish() {
    let (tool, why) = best_for(Job::HandOff, &[Editor::Premiere]);
    assert_eq!(tool, Editor::Premiere);
    assert!(why.contains("for you to finish"));
}

#[test]
fn owning_premiere_does_not_change_how_the_simple_jobs_are_done() {
    // Opening Premiere to normalise audio is slower and no better.
    for job in [Job::Loudness, Job::Trim, Job::Captions, Job::Crop] {
        let (tool, _) = best_for(job, &[Editor::Premiere, Editor::AfterEffects, Editor::Photoshop]);
        assert_eq!(tool, Editor::Ffmpeg, "{job:?}");
    }
}

#[test]
fn final_cut_can_be_prepared_for_but_not_driven() {
    assert!(!Editor::FinalCut.drivable());
    assert!(Editor::FinalCut.how().contains("Atlas prepares, you finish"));
    assert!(Editor::Resolve.drivable());
    assert!(Editor::Premiere.how().contains("ExtendScript"));
}

#[test]
fn atlas_says_when_it_used_something_your_friend_would_not_have() {
    // A result that needed Premiere isn't a result they can reproduce.
    let cfg = EditorConfig::default();
    let said = used(Editor::Premiere, Job::HandOff, &cfg);
    assert!(said.contains("Used Premiere Pro"));
    assert!(said.contains("On a machine without it I'd have done it with ffmpeg"));

    assert_eq!(used(Editor::Ffmpeg, Job::Trim, &cfg), "", "nothing to say about the default");
}

#[test]
fn ffmpeg_being_the_floor_is_not_configurable() {
    let parsed: EditorConfig =
        serde_yaml::from_str("use_what_you_have: true\nffmpeg_is_the_floor: false\n").unwrap();
    assert!(parsed.ffmpeg_is_the_floor);
}

#[test]
fn setup_knows_where_to_look_for_each() {
    assert!(where_to_look(Editor::Premiere).contains("Adobe"));
    assert!(where_to_look(Editor::Resolve).contains("Blackmagic"));
}
